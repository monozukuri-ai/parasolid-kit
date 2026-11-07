//! Validated project-owned built-in schema profiles, exact-key selection and
//! the schema families derived from them.

use std::collections::{BTreeMap, BTreeSet};

use crate::{ErrorDetails, ErrorKind, ParseError};

use super::profiles::topsolid::{TopSolidV6PsolidCatalogProvider, TopSolidV6PsolidSchemaProvider};
use super::{
    FieldType, SchemaKey, SchemaProvider, SchemaProviderProvenance, SchemaSource, SchemaTypeLookup,
    TypeDefinition, WireDecodeClass, digest::sha256_hex,
};

const MAX_PROFILE_ID_BYTES: usize = 128;
const MAX_PRODUCER_SCOPE_BYTES: usize = 128;
const MAX_FIELDS_PER_TYPE: usize = 4_096;

/// Declared completeness of one project-owned built-in profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinProfileCoverage {
    /// Only node types proven by the recorded evidence are supplied.
    VerifiedSubset,
    /// The reviewed declarations shared by the exact profiles of one schema
    /// family, selected for a key whose modeller build has no exact profile.
    FamilyInferred,
}

impl BuiltinProfileCoverage {
    /// Return the stable value used by bindings and reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VerifiedSubset => "verified_subset",
            Self::FamilyInferred => "family_inferred",
        }
    }
}

/// One reviewed exact profile contributing to a derived schema family.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FamilyMember {
    /// Stable identifier of the exact profile.
    pub profile_id: String,
    /// Revision of the exact profile.
    pub revision: u32,
    /// Declared digest of the exact profile.
    pub profile_sha256: String,
}

/// Reviewed provenance declared by one compiled built-in profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinProfileMetadata {
    /// Stable project-owned identifier, for example `onshape-sch30000-r1`.
    pub profile_id: String,
    /// Monotonic profile revision, beginning at one.
    pub revision: u32,
    /// Numeric provider-schema component used for type lookup.
    pub provider_schema: String,
    /// Producer scope supported by the evidence, for example `Onshape`.
    pub producer_scope: String,
    /// Whether the profile is complete or covers only reviewed types.
    pub coverage: BuiltinProfileCoverage,
    /// Optional digest of the evidence manifest used during promotion.
    pub evidence_manifest_sha256: Option<String>,
    /// Digest of the reviewed canonical profile source.
    pub profile_sha256: String,
}

/// One validated built-in profile, independent of caller-owned schema catalogs.
///
/// An exact profile accepts complete transmit keys. A family profile, built by
/// [`BuiltinSchemaProfile::new_family`], accepts every key of one schema family
/// and carries the members it was merged from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinSchemaProfile {
    metadata: BuiltinProfileMetadata,
    accepted_schema_keys: Vec<SchemaKey>,
    definitions: BTreeMap<u16, TypeDefinition>,
    unsupported_base_types: BTreeSet<u16>,
    absent_base_types: BTreeSet<u16>,
    family_id: Option<String>,
    members: Vec<FamilyMember>,
    base_members: Vec<FamilyMember>,
    conflicting_types: BTreeSet<u16>,
}

impl BuiltinSchemaProfile {
    /// Validate and construct one built-in profile.
    ///
    /// The supplied SHA-256 values are promotion metadata produced by developer
    /// tooling. The core validates their representation and retains them; it does
    /// not read a source file or contact an external service to recompute them.
    ///
    /// # Errors
    ///
    /// Returns `schema.invalid_builtin_profile` for invalid metadata, duplicate
    /// definitions or keys, embedded-base keys, or unsafe field layouts.
    pub fn new(
        metadata: BuiltinProfileMetadata,
        accepted_schema_keys: Vec<String>,
        definitions: Vec<TypeDefinition>,
    ) -> Result<Self, ParseError> {
        validate_metadata(&metadata)?;
        let accepted_schema_keys = validate_schema_keys(&metadata, accepted_schema_keys, false)?;
        let definitions = validate_definitions(definitions)?;
        Ok(Self {
            metadata,
            accepted_schema_keys,
            definitions,
            unsupported_base_types: BTreeSet::new(),
            absent_base_types: BTreeSet::new(),
            family_id: None,
            members: Vec::new(),
            base_members: Vec::new(),
            conflicting_types: BTreeSet::new(),
        })
    }

    /// Construct a partial base profile for explicitly allowlisted embedded keys.
    ///
    /// Missing definitions are unknown unless listed as present-but-unsupported
    /// or confirmed absent. Only confirmed absence permits a full embedded
    /// declaration. This constructor does not register the profile for default
    /// parsing or establish B-Rep roles for its effective definitions.
    ///
    /// # Errors
    ///
    /// Rejects non-embedded keys, invalid definitions, duplicate declarations,
    /// and contradictory base-type membership.
    pub fn new_embedded(
        metadata: BuiltinProfileMetadata,
        accepted_schema_keys: Vec<String>,
        definitions: Vec<TypeDefinition>,
        unsupported_base_types: Vec<u16>,
        absent_base_types: Vec<u16>,
    ) -> Result<Self, ParseError> {
        validate_metadata(&metadata)?;
        let accepted_schema_keys = validate_schema_keys(&metadata, accepted_schema_keys, true)?;
        let definitions = validate_definitions(definitions)?;
        let mut declared: BTreeSet<_> = definitions.keys().copied().collect();
        for node_type in unsupported_base_types.iter().chain(&absent_base_types) {
            if *node_type <= 1 || !declared.insert(*node_type) {
                return Err(invalid_profile(
                    "base_type_membership",
                    node_type.to_string(),
                    "base membership must be unique, non-contradictory, and greater than one",
                ));
            }
        }
        Ok(Self {
            metadata,
            accepted_schema_keys,
            definitions,
            unsupported_base_types: unsupported_base_types.into_iter().collect(),
            absent_base_types: absent_base_types.into_iter().collect(),
            family_id: None,
            members: Vec::new(),
            base_members: Vec::new(),
            conflicting_types: BTreeSet::new(),
        })
    }

    /// Derive the profile of one schema family from the exact profiles of its
    /// provider schema.
    ///
    /// A transmit key is `SCH_<modeller>_<effective>[_<base>]`. The effective
    /// schema and the base fix the node layouts; the modeller component is the
    /// producer's Parasolid build number, so a family profile covers builds that
    /// have no exact profile. `contributors` are every exact profile whose
    /// provider schema is the family's: the ones accepting a key of this family
    /// are its members (at least one is required), the others are base members
    /// whose declarations describe the same provider schema (for an embedded
    /// family, the shared base; version differences arrive as each stream's
    /// embedded edits). Definitions shared by two contributors must have one
    /// layout, and a type declared by one cannot be recorded as absent or
    /// unsupported by another. A type that violates either rule is excluded from
    /// the family and stays unknown, so it still fails closed. The profile ID is
    /// `family-<family id>`; its digest hashes the sorted member and base-member
    /// identities.
    ///
    /// # Errors
    ///
    /// Returns `schema.invalid_builtin_profile` when no contributor accepts a key
    /// of the family, the contributors span more than one provider schema, a
    /// contributor is itself a family, or no reviewed definition remains.
    #[allow(clippy::too_many_lines)]
    pub fn new_family(family_id: &str, contributors: &[&Self]) -> Result<Self, ParseError> {
        let mut provider_schemas = BTreeSet::new();
        let mut members = Vec::new();
        let mut base_members = Vec::new();
        for contributor in contributors {
            if contributor.family_id.is_some() {
                return Err(invalid_profile(
                    "family",
                    &contributor.metadata.profile_id,
                    "a family profile cannot contribute to another family",
                ));
            }
            provider_schemas.insert(contributor.metadata.provider_schema.clone());
            let keys = contributor
                .accepted_schema_keys
                .iter()
                .filter(|key| key.family_id() == family_id)
                .collect::<Vec<_>>();
            if keys.is_empty() {
                base_members.push(*contributor);
            } else {
                if keys
                    .iter()
                    .any(|key| key.provider_schema() != contributor.metadata.provider_schema)
                {
                    return Err(invalid_profile(
                        "family",
                        family_id,
                        "family keys must use the contributor's provider schema",
                    ));
                }
                members.push(*contributor);
            }
        }
        if members.is_empty() || provider_schemas.len() != 1 {
            return Err(invalid_profile(
                "family",
                family_id,
                "a schema family needs exact profiles of that family sharing one provider schema",
            ));
        }
        let provider_schema = provider_schemas.into_iter().next().unwrap_or_default();

        let sort = |profiles: &mut Vec<&Self>| {
            profiles
                .sort_by(|left, right| left.metadata.profile_id.cmp(&right.metadata.profile_id));
            profiles.dedup_by(|left, right| left.metadata.profile_id == right.metadata.profile_id);
        };
        sort(&mut members);
        sort(&mut base_members);

        let mut definitions: BTreeMap<u16, TypeDefinition> = BTreeMap::new();
        let mut unsupported = BTreeSet::new();
        let mut absent = BTreeSet::new();
        let mut conflicting = BTreeSet::new();
        for member in members.iter().chain(&base_members) {
            for definition in member.definitions.values() {
                match definitions.get(&definition.node_type) {
                    None => {
                        definitions.insert(definition.node_type, definition.clone());
                    }
                    Some(existing)
                        if existing.fields == definition.fields
                            && existing.variable == definition.variable => {}
                    Some(_) => {
                        conflicting.insert(definition.node_type);
                    }
                }
            }
            unsupported.extend(member.unsupported_base_types.iter().copied());
            absent.extend(member.absent_base_types.iter().copied());
        }
        conflicting.extend(
            definitions
                .keys()
                .filter(|node_type| unsupported.contains(node_type) || absent.contains(node_type))
                .copied(),
        );
        conflicting.extend(unsupported.intersection(&absent).copied());
        for node_type in &conflicting {
            definitions.remove(node_type);
            unsupported.remove(node_type);
            absent.remove(node_type);
        }
        if definitions.is_empty() {
            return Err(invalid_profile(
                "definitions",
                "0",
                "schema family has no shared reviewed definition",
            ));
        }

        let identities = |profiles: &[&Self]| {
            profiles
                .iter()
                .map(|profile| FamilyMember {
                    profile_id: profile.metadata.profile_id.clone(),
                    revision: profile.metadata.revision,
                    profile_sha256: profile.metadata.profile_sha256.clone(),
                })
                .collect::<Vec<_>>()
        };
        let family_members = identities(&members);
        let family_base_members = identities(&base_members);
        let metadata = BuiltinProfileMetadata {
            profile_id: format!("family-{family_id}"),
            revision: 1,
            provider_schema,
            producer_scope: format!(
                "schema family {family_id}: {} reviewed profiles, {} base profiles",
                family_members.len(),
                family_base_members.len()
            ),
            coverage: BuiltinProfileCoverage::FamilyInferred,
            evidence_manifest_sha256: None,
            profile_sha256: family_digest(family_id, &family_members, &family_base_members),
        };
        validate_metadata(&metadata)?;
        Ok(Self {
            metadata,
            accepted_schema_keys: Vec::new(),
            definitions,
            unsupported_base_types: unsupported,
            absent_base_types: absent,
            family_id: Some(family_id.to_owned()),
            members: family_members,
            base_members: family_base_members,
            conflicting_types: conflicting,
        })
    }

    /// Return reviewed profile metadata.
    #[must_use]
    pub const fn metadata(&self) -> &BuiltinProfileMetadata {
        &self.metadata
    }

    /// Iterate over exact accepted transmit schema keys in lexical order.
    ///
    /// A family profile accepts keys by family instead and returns no exact key.
    #[must_use]
    pub fn accepted_schema_keys(&self) -> impl ExactSizeIterator<Item = &SchemaKey> {
        self.accepted_schema_keys.iter()
    }

    /// Return the schema family this profile was derived for, if it is a family profile.
    #[must_use]
    pub fn family_id(&self) -> Option<&str> {
        self.family_id.as_deref()
    }

    /// Return the exact profiles of this family merged into the family profile, sorted by ID.
    #[must_use]
    pub fn members(&self) -> &[FamilyMember] {
        &self.members
    }

    /// Return the exact profiles of other families that contributed declarations
    /// of the same provider schema, sorted by ID.
    #[must_use]
    pub fn base_members(&self) -> &[FamilyMember] {
        &self.base_members
    }

    /// Iterate over node types excluded from a family because its members disagree.
    #[must_use]
    pub fn conflicting_types(&self) -> impl ExactSizeIterator<Item = u16> + '_ {
        self.conflicting_types.iter().copied()
    }

    /// Iterate over reviewed definitions in ascending node-type order.
    #[must_use]
    pub fn definitions(&self) -> impl ExactSizeIterator<Item = &TypeDefinition> {
        self.definitions.values()
    }

    /// Return one reviewed definition.
    #[must_use]
    pub fn definition(&self, node_type: u16) -> Option<&TypeDefinition> {
        self.definitions.get(&node_type)
    }

    /// Iterate over confirmed base types whose definitions remain unsupported.
    #[must_use]
    pub fn unsupported_base_types(&self) -> impl ExactSizeIterator<Item = u16> + '_ {
        self.unsupported_base_types.iter().copied()
    }

    /// Iterate over node types confirmed absent from the selected base schema.
    #[must_use]
    pub fn absent_base_types(&self) -> impl ExactSizeIterator<Item = u16> + '_ {
        self.absent_base_types.iter().copied()
    }

    /// Return whether the complete key is explicitly allowlisted, or belongs to
    /// this profile's schema family for a family profile.
    #[must_use]
    pub fn accepts_schema_key(&self, schema_key: &SchemaKey) -> bool {
        match &self.family_id {
            Some(family_id) => {
                *family_id == schema_key.family_id()
                    && self.metadata.provider_schema == schema_key.provider_schema()
            }
            None => self
                .accepted_schema_keys
                .binary_search_by(|candidate| candidate.raw().cmp(schema_key.raw()))
                .is_ok(),
        }
    }
}

/// SHA-256 of the canonical contributor lists of one schema family:
/// `{"base_members":[["<profile_id>",<revision>,"<sha256>"],...],"family":"<id>",
/// "members":[...]}`, each list sorted by profile ID.
fn family_digest(
    family_id: &str,
    members: &[FamilyMember],
    base_members: &[FamilyMember],
) -> String {
    fn push_list(canonical: &mut String, members: &[FamilyMember]) {
        canonical.push('[');
        for (index, member) in members.iter().enumerate() {
            if index > 0 {
                canonical.push(',');
            }
            canonical.push_str("[\"");
            canonical.push_str(&member.profile_id);
            canonical.push_str("\",");
            canonical.push_str(&member.revision.to_string());
            canonical.push_str(",\"");
            canonical.push_str(&member.profile_sha256);
            canonical.push_str("\"]");
        }
        canonical.push(']');
    }
    let mut canonical = String::from("{\"base_members\":");
    push_list(&mut canonical, base_members);
    canonical.push_str(",\"family\":\"");
    canonical.push_str(family_id);
    canonical.push_str("\",\"members\":");
    push_list(&mut canonical, members);
    canonical.push('}');
    sha256_hex(canonical.as_bytes())
}

/// Registry of reviewed profiles indexed by stable ID and exact schema key,
/// with one derived family profile per schema family of the exact keys.
#[derive(Debug, Clone, Default)]
pub struct BuiltinProfileRegistry {
    profiles: Vec<BuiltinSchemaProfile>,
    profile_indices: BTreeMap<String, usize>,
    schema_key_indices: BTreeMap<String, usize>,
    families: Vec<BuiltinSchemaProfile>,
    family_indices: BTreeMap<String, usize>,
    topsolid_catalogs: Option<TopSolidV6PsolidCatalogProvider>,
}

impl BuiltinProfileRegistry {
    /// Return the compiled profiles eligible for default exact-key selection.
    ///
    /// `Default` and `new` remain caller-managed registries. This shared registry
    /// contains only profiles validated for runtime use.
    ///
    /// # Errors
    ///
    /// Returns `schema.invalid_builtin_profile` for an inconsistent compiled table.
    pub fn compiled() -> Result<&'static Self, ParseError> {
        static REGISTRY: std::sync::OnceLock<Result<BuiltinProfileRegistry, ParseError>> =
            std::sync::OnceLock::new();
        REGISTRY
            .get_or_init(|| {
                let mut profiles = vec![
                    super::profiles::onshape_sch30000()?,
                    super::profiles::onshape_sch13006()?,
                    super::profiles::icad_sch30000_13006()?,
                    super::profiles::icad_sch34101_13006()?,
                    super::profiles::solidworks_sch37102_13006()?,
                    super::profiles::onshape_sch37102_13006()?,
                ];
                for raw_key in super::profiles::topsolid::SUPPORTED_SCHEMA_KEYS {
                    let key = SchemaKey::parse(raw_key)?;
                    let profile = super::profiles::topsolid::topsolid_v6_psolid_base_profile(&key)
                        .map_err(|error| {
                            invalid_profile(
                                "topsolid_profile",
                                error.to_string(),
                                "compiled TopSolid profile could not be loaded",
                            )
                        })?
                        .ok_or_else(|| {
                            invalid_profile(
                                "topsolid_schema_key",
                                raw_key,
                                "compiled TopSolid schema key has no base profile",
                            )
                        })?;
                    profiles.push(profile);
                }
                profiles.extend(super::profiles::icad_legacy_13006()?);
                profiles.extend(super::profiles::icad_legacy_standard()?);
                profiles.extend(super::profiles::solidworks_partitions_13006()?);
                profiles.extend(super::profiles::transmit_13006()?);
                profiles.extend(super::profiles::legacy_standard_keys()?);
                let mut registry = Self::new(profiles)?;
                registry.topsolid_catalogs =
                    Some(TopSolidV6PsolidCatalogProvider::new().map_err(|error| {
                        invalid_profile(
                            "topsolid_catalog",
                            error.to_string(),
                            "compiled TopSolid catalog provider could not be loaded",
                        )
                    })?);
                Ok(registry)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Validate and index all supplied profiles.
    ///
    /// # Errors
    ///
    /// Returns `schema.invalid_builtin_profile` when an ID or exact schema key is
    /// claimed by more than one profile.
    pub fn new(profiles: Vec<BuiltinSchemaProfile>) -> Result<Self, ParseError> {
        let mut registry = Self::default();
        for profile in profiles {
            registry.insert(profile)?;
        }
        Ok(registry)
    }

    /// Insert one validated exact profile without replacing an existing claim,
    /// then rebuild the derived family profiles.
    ///
    /// # Errors
    ///
    /// Returns `schema.invalid_builtin_profile` for duplicate IDs or keys, a
    /// family profile, or a family that cannot be derived from its members.
    pub fn insert(&mut self, profile: BuiltinSchemaProfile) -> Result<(), ParseError> {
        let profile_id = &profile.metadata.profile_id;
        if profile.family_id.is_some() {
            return Err(invalid_profile(
                "profile_id",
                profile_id,
                "family profiles are derived by the registry and cannot be inserted",
            ));
        }
        if self.profile_indices.contains_key(profile_id) {
            return Err(invalid_profile(
                "profile_id",
                profile_id,
                "built-in profile identifier is already registered",
            ));
        }
        for schema_key in &profile.accepted_schema_keys {
            if self.schema_key_indices.contains_key(schema_key.raw()) {
                return Err(invalid_profile(
                    "schema_key",
                    schema_key.raw(),
                    "complete schema key is already claimed by another built-in profile",
                ));
            }
        }

        let index = self.profiles.len();
        self.profile_indices.insert(profile_id.clone(), index);
        for schema_key in &profile.accepted_schema_keys {
            self.schema_key_indices
                .insert(schema_key.raw().to_owned(), index);
        }
        self.profiles.push(profile);
        self.rebuild_families()
    }

    fn rebuild_families(&mut self) -> Result<(), ParseError> {
        // Family id -> provider schema of its keys.
        let mut grouped: BTreeMap<String, String> = BTreeMap::new();
        for profile in &self.profiles {
            if profile.metadata.producer_scope.starts_with("TopSolid") {
                continue;
            }
            for schema_key in &profile.accepted_schema_keys {
                grouped
                    .entry(schema_key.family_id())
                    .or_insert_with(|| schema_key.provider_schema().to_owned());
            }
        }
        let mut families = Vec::with_capacity(grouped.len());
        let mut family_indices = BTreeMap::new();
        for (family_id, provider_schema) in grouped {
            // Every exact profile of the provider schema contributes its reviewed
            // declarations; the ones accepting a key of this family are its members.
            let contributors = self
                .profiles
                .iter()
                .filter(|profile| {
                    profile.metadata.provider_schema == provider_schema
                        && !profile.metadata.producer_scope.starts_with("TopSolid")
                })
                .collect::<Vec<_>>();
            let family = BuiltinSchemaProfile::new_family(&family_id, &contributors)?;
            if self
                .profile_indices
                .contains_key(&family.metadata.profile_id)
            {
                return Err(invalid_profile(
                    "profile_id",
                    &family.metadata.profile_id,
                    "schema family identifier collides with a registered profile",
                ));
            }
            family_indices.insert(family_id, families.len());
            families.push(family);
        }
        self.families = families;
        self.family_indices = family_indices;
        Ok(())
    }

    /// Return the number of registered exact profiles.
    #[must_use]
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// Return whether no exact profile has been registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    /// Iterate over exact profiles in insertion order.
    #[must_use]
    pub fn profiles(&self) -> impl ExactSizeIterator<Item = &BuiltinSchemaProfile> {
        self.profiles.iter()
    }

    /// Iterate over derived family profiles in family-ID order.
    #[must_use]
    pub fn families(&self) -> impl ExactSizeIterator<Item = &BuiltinSchemaProfile> {
        self.families.iter()
    }

    /// Return one exact or family profile by stable ID.
    #[must_use]
    pub fn profile(&self, profile_id: &str) -> Option<&BuiltinSchemaProfile> {
        self.profile_indices
            .get(profile_id)
            .and_then(|index| self.profiles.get(*index))
            .or_else(|| {
                self.families
                    .iter()
                    .find(|family| family.metadata.profile_id == profile_id)
            })
    }

    /// Return the exact profile which explicitly accepts the complete schema key.
    #[must_use]
    pub fn profile_for_key(&self, schema_key: &SchemaKey) -> Option<&BuiltinSchemaProfile> {
        self.schema_key_indices
            .get(schema_key.raw())
            .and_then(|index| self.profiles.get(*index))
    }

    /// Return the family profile derived for the key's schema family, when any
    /// exact profile of that family is registered.
    #[must_use]
    pub fn family_for_key(&self, schema_key: &SchemaKey) -> Option<&BuiltinSchemaProfile> {
        self.family_indices
            .get(&schema_key.family_id())
            .and_then(|index| self.families.get(*index))
            .filter(|family| family.accepts_schema_key(schema_key))
    }

    /// Return a provider for one complete schema key: its exact profile when one
    /// is allowlisted, otherwise the profile derived for its schema family.
    ///
    /// The provider accepts only the requested key. Its provenance reports
    /// `verified_subset` coverage for an exact profile and `family_inferred`
    /// for a family profile. An exact profile is completed by its family: a
    /// node type the exact profile neither declares nor classifies is looked
    /// up in the family profile, so the same schema knowledge serves a reviewed
    /// build and its neighbours.
    #[must_use]
    pub fn provider_for_key(&self, schema_key: &SchemaKey) -> Option<BuiltinSchemaProvider<'_>> {
        let family = self.family_for_key(schema_key);
        let profile = self.profile_for_key(schema_key).or(family)?;
        Some(BuiltinSchemaProvider {
            profile,
            family: family.filter(|family| !std::ptr::eq(*family, profile)),
            selected_schema_key: schema_key.raw().to_owned(),
            topsolid: self
                .topsolid_catalogs
                .as_ref()
                .and_then(|catalogs| catalogs.provider_for_key(schema_key)),
        })
    }
}

/// Borrowed provider selected for one complete schema key from an exact or
/// family built-in profile.
#[derive(Debug, Clone)]
pub struct BuiltinSchemaProvider<'a> {
    profile: &'a BuiltinSchemaProfile,
    family: Option<&'a BuiltinSchemaProfile>,
    selected_schema_key: String,
    topsolid: Option<TopSolidV6PsolidSchemaProvider<'a>>,
}

impl BuiltinSchemaProvider<'_> {
    /// Return the selected built-in profile.
    #[must_use]
    pub const fn profile(&self) -> &BuiltinSchemaProfile {
        self.profile
    }

    /// Return the family profile that completes an exact profile's declarations,
    /// or `None` when the selected profile is the family itself.
    #[must_use]
    pub const fn family(&self) -> Option<&BuiltinSchemaProfile> {
        self.family
    }

    /// Return the complete key this provider was selected for.
    #[must_use]
    pub fn selected_schema_key(&self) -> &str {
        &self.selected_schema_key
    }
}

fn lookup_in(profile: &BuiltinSchemaProfile, node_type: u16) -> SchemaTypeLookup<'_> {
    if let Some(definition) = profile.definition(node_type) {
        SchemaTypeLookup::Defined(definition)
    } else if profile.unsupported_base_types.contains(&node_type) {
        SchemaTypeLookup::PresentUnsupported
    } else if profile.absent_base_types.contains(&node_type) {
        SchemaTypeLookup::Absent
    } else {
        SchemaTypeLookup::Unknown
    }
}

impl SchemaProvider for BuiltinSchemaProvider<'_> {
    fn contains_schema(&self, schema: &str) -> bool {
        schema == self.profile.metadata.provider_schema
            || self
                .topsolid
                .as_ref()
                .is_some_and(|provider| provider.contains_schema(schema))
    }

    fn type_definition(&self, schema: &str, node_type: u16) -> Option<&TypeDefinition> {
        if !self.contains_schema(schema) {
            return None;
        }
        if self
            .topsolid
            .as_ref()
            .is_some_and(|provider| provider.contains_schema(schema))
        {
            return self
                .topsolid
                .as_ref()
                .and_then(|provider| provider.type_definition(schema, node_type));
        }
        self.profile
            .definition(node_type)
            .or_else(|| self.family.and_then(|family| family.definition(node_type)))
    }

    fn lookup_type(&self, schema: &str, node_type: u16) -> SchemaTypeLookup<'_> {
        if !self.contains_schema(schema) {
            return SchemaTypeLookup::Unknown;
        }
        if let Some(provider) = self
            .topsolid
            .as_ref()
            .filter(|provider| provider.contains_schema(schema))
        {
            return provider.lookup_type(schema, node_type);
        }
        match lookup_in(self.profile, node_type) {
            SchemaTypeLookup::Unknown => self.family.map_or(SchemaTypeLookup::Unknown, |family| {
                lookup_in(family, node_type)
            }),
            known => known,
        }
    }

    fn supports_schema_key(&self, schema_key: &SchemaKey) -> bool {
        self.selected_schema_key == schema_key.raw()
            && self.profile.accepts_schema_key(schema_key)
            && self.contains_schema(schema_key.provider_schema())
    }

    fn provenance(&self) -> SchemaProviderProvenance<'_> {
        if let Some(provider) = &self.topsolid {
            return provider.provenance();
        }
        SchemaProviderProvenance::Builtin {
            profile_id: &self.profile.metadata.profile_id,
            profile_revision: self.profile.metadata.revision,
            schema_key: &self.selected_schema_key,
            coverage: self.profile.metadata.coverage,
            profile_sha256: &self.profile.metadata.profile_sha256,
        }
    }
}

fn validate_metadata(metadata: &BuiltinProfileMetadata) -> Result<(), ParseError> {
    if metadata.profile_id.is_empty()
        || metadata.profile_id.len() > MAX_PROFILE_ID_BYTES
        || !metadata.profile_id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
    {
        return Err(invalid_profile(
            "profile_id",
            &metadata.profile_id,
            "profile identifier must use 1-128 lowercase ASCII identifier bytes",
        ));
    }
    if metadata.revision == 0 {
        return Err(invalid_profile(
            "profile_revision",
            metadata.revision.to_string(),
            "profile revision must be greater than zero",
        ));
    }
    if metadata.provider_schema.is_empty()
        || !metadata
            .provider_schema
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid_profile(
            "provider_schema",
            &metadata.provider_schema,
            "provider schema must contain only ASCII digits",
        ));
    }
    if metadata.producer_scope.is_empty()
        || metadata.producer_scope.len() > MAX_PRODUCER_SCOPE_BYTES
        || !metadata
            .producer_scope
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' ')
    {
        return Err(invalid_profile(
            "producer_scope",
            &metadata.producer_scope,
            "producer scope must use 1-128 printable ASCII bytes",
        ));
    }
    validate_sha256("profile_sha256", &metadata.profile_sha256)?;
    if let Some(digest) = &metadata.evidence_manifest_sha256 {
        validate_sha256("evidence_manifest_sha256", digest)?;
    }
    Ok(())
}

fn validate_schema_keys(
    metadata: &BuiltinProfileMetadata,
    accepted_schema_keys: Vec<String>,
    embedded: bool,
) -> Result<Vec<SchemaKey>, ParseError> {
    if accepted_schema_keys.is_empty() {
        return Err(invalid_profile(
            "schema_key",
            "",
            "built-in profile must accept at least one exact schema key",
        ));
    }
    let mut parsed = Vec::with_capacity(accepted_schema_keys.len());
    for raw in accepted_schema_keys {
        let schema_key = SchemaKey::parse(&raw).map_err(|_| {
            invalid_profile(
                "schema_key",
                &raw,
                "built-in profile contains an invalid schema key",
            )
        })?;
        if schema_key.base().is_some() != embedded {
            return Err(invalid_profile(
                "schema_key",
                &raw,
                "schema key must match the constructor's standard or embedded mode",
            ));
        }
        if schema_key.provider_schema() != metadata.provider_schema {
            return Err(invalid_profile(
                "schema_key",
                &raw,
                "schema key provider component does not match profile metadata",
            ));
        }
        parsed.push(schema_key);
    }
    parsed.sort_by(|left, right| left.raw().cmp(right.raw()));
    if parsed.windows(2).any(|pair| pair[0].raw() == pair[1].raw()) {
        return Err(invalid_profile(
            "schema_key",
            parsed
                .windows(2)
                .find(|pair| pair[0].raw() == pair[1].raw())
                .map_or("", |pair| pair[0].raw()),
            "built-in profile contains a duplicate exact schema key",
        ));
    }
    Ok(parsed)
}

fn validate_definitions(
    definitions: Vec<TypeDefinition>,
) -> Result<BTreeMap<u16, TypeDefinition>, ParseError> {
    if definitions.is_empty() {
        return Err(invalid_profile(
            "definitions",
            "0",
            "built-in profile must contain at least one reviewed type definition",
        ));
    }
    let mut validated = BTreeMap::new();
    for definition in definitions {
        validate_definition(&definition)?;
        let node_type = definition.node_type;
        if validated.insert(node_type, definition).is_some() {
            return Err(invalid_profile(
                "node_type",
                node_type.to_string(),
                "built-in profile contains a duplicate node type",
            ));
        }
    }
    Ok(validated)
}

fn validate_definition(definition: &TypeDefinition) -> Result<(), ParseError> {
    if definition.node_type <= 1 {
        return Err(invalid_profile(
            "node_type",
            definition.node_type.to_string(),
            "built-in node type must be greater than one",
        ));
    }
    if definition.name.is_empty() {
        return Err(invalid_profile(
            "node_name",
            "",
            "built-in node definition must have a project-owned name",
        ));
    }
    if definition.source != SchemaSource::Base {
        return Err(invalid_profile(
            "schema_source",
            definition.source.as_str(),
            "built-in definitions must enter the parser as base definitions",
        ));
    }
    if definition.fields.len() > MAX_FIELDS_PER_TYPE {
        return Err(invalid_profile(
            "fields_per_type",
            definition.fields.len().to_string(),
            "built-in definition exceeds the field-count bound",
        ));
    }

    let variable_positions = definition
        .fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| (field.element_count == 1).then_some(index))
        .collect::<Vec<_>>();
    if variable_positions.len() > 1
        || variable_positions
            .first()
            .is_some_and(|position| *position + 1 != definition.fields.len())
        || definition.variable == variable_positions.is_empty()
    {
        return Err(invalid_profile(
            "variable_field",
            definition.node_type.to_string(),
            "variable field must be the sole final field and match the type flag",
        ));
    }

    let mut field_names = BTreeSet::new();
    for (index, field) in definition.fields.iter().enumerate() {
        if field.name.is_empty() {
            return Err(invalid_profile(
                "field_name",
                index.to_string(),
                "built-in field must have a project-owned name",
            ));
        }
        if !field_names.insert(field.name.as_str()) {
            return Err(invalid_profile(
                "field_name",
                &field.name,
                "built-in definition contains a duplicate field name",
            ));
        }
        if field.pointer_class != 0 && field.field_type != FieldType::PointerIndex {
            return Err(invalid_profile(
                "pointer_class",
                field.pointer_class.to_string(),
                "non-zero pointer class requires the pointer-index codec",
            ));
        }
        if !field.transmitted && field.element_count != 1 {
            return Err(invalid_profile(
                "field_transmitted",
                index.to_string(),
                "only a retained variable field may be non-transmitted",
            ));
        }
        if field.transmitted && WireDecodeClass::from_field_type(field.field_type).is_none() {
            return Err(invalid_profile(
                "field_type",
                field.field_type.code(),
                "opaque pointers may not be transmitted by a built-in profile",
            ));
        }
    }
    Ok(())
}

fn validate_sha256(field: &'static str, value: &str) -> Result<(), ParseError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid_profile(
            field,
            value,
            "digest must be 64 lowercase hexadecimal SHA-256 characters",
        ));
    }
    Ok(())
}

fn invalid_profile(
    field: &'static str,
    value: impl Into<String>,
    message: &'static str,
) -> ParseError {
    ParseError::new(
        ErrorKind::InvalidBuiltinProfile,
        0,
        message,
        ErrorDetails::InvalidText {
            field,
            value: value.into(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DocumentLimits, ErrorDetails, ErrorKind, FieldDefinition, FieldValue,
        SchemaProviderResolution, parse_xb, parse_xt, write_xb,
    };

    const SCHEMA_KEY: &str = "SCH_1000000_90000";
    const NEAR_SCHEMA_KEY: &str = "SCH_1000001_90000";

    fn metadata(profile_id: &str) -> BuiltinProfileMetadata {
        BuiltinProfileMetadata {
            profile_id: profile_id.to_owned(),
            revision: 1,
            provider_schema: "90000".to_owned(),
            producer_scope: "Synthetic tests".to_owned(),
            coverage: BuiltinProfileCoverage::VerifiedSubset,
            evidence_manifest_sha256: None,
            profile_sha256: "0".repeat(64),
        }
    }

    fn definition(node_type: u16) -> TypeDefinition {
        TypeDefinition::from_fields(
            node_type,
            format!("PROJECT_TYPE_{node_type}"),
            "Project-owned synthetic definition",
            vec![FieldDefinition {
                name: "field_0".to_owned(),
                field_type: FieldType::Integer,
                pointer_class: 0,
                element_count: 0,
                transmitted: true,
            }],
            SchemaSource::Base,
        )
    }

    fn profile_with(
        profile_id: &str,
        schema_key: &str,
        node_type: u16,
    ) -> Result<BuiltinSchemaProfile, ParseError> {
        BuiltinSchemaProfile::new(
            metadata(profile_id),
            vec![schema_key.to_owned()],
            vec![definition(node_type)],
        )
    }

    fn registry() -> Result<BuiltinProfileRegistry, ParseError> {
        let profile = profile_with("synthetic-sch90000-r1", SCHEMA_KEY, 42)?;
        BuiltinProfileRegistry::new(vec![profile])
    }

    fn xt_payload(schema_key: &str, node_type: u16, value: i32) -> Vec<u8> {
        let modeller = ": TRANSMIT FILE created by modeller version 1000000";
        format!(
            "T{} {}{} {}0 {node_type} 1 {value} 1 0 ",
            modeller.len(),
            modeller,
            schema_key.len(),
            schema_key,
        )
        .into_bytes()
    }

    fn xb_payload(schema_key: &str, node_type: u16, value: i32) -> Vec<u8> {
        let modeller = b": TRANSMIT FILE created by modeller version 1000000";
        let mut output = b"PS\0\0".to_vec();
        output.extend_from_slice(
            &u16::try_from(modeller.len())
                .unwrap_or_default()
                .to_be_bytes(),
        );
        output.extend_from_slice(modeller);
        output.extend_from_slice(
            &i32::try_from(schema_key.len())
                .unwrap_or_default()
                .to_be_bytes(),
        );
        output.extend_from_slice(schema_key.as_bytes());
        output.extend_from_slice(&0_i32.to_be_bytes());
        output.extend_from_slice(&i16::try_from(node_type).unwrap_or_default().to_be_bytes());
        output.extend_from_slice(&2_i16.to_be_bytes());
        output.extend_from_slice(&value.to_be_bytes());
        output.extend_from_slice(&1_i16.to_be_bytes());
        output.extend_from_slice(&1_i16.to_be_bytes());
        output
    }

    #[test]
    fn indexes_profiles_by_stable_id_and_exact_key() {
        let registry = registry();
        assert!(registry.is_ok(), "{registry:?}");
        if let Ok(registry) = registry {
            assert_eq!(registry.len(), 1);
            assert!(registry.profile("synthetic-sch90000-r1").is_some());

            let exact = SchemaKey::parse(SCHEMA_KEY);
            assert!(exact.is_ok(), "{exact:?}");
            if let Ok(exact) = exact {
                let provider = registry.provider_for_key(&exact);
                assert!(provider.is_some());
                if let Some(provider) = provider {
                    assert_eq!(provider.selected_schema_key(), SCHEMA_KEY);
                    assert_eq!(
                        provider.profile().metadata().coverage.as_str(),
                        "verified_subset"
                    );
                    assert!(provider.supports_schema_key(&exact));
                    assert!(provider.type_definition("90000", 42).is_some());
                }
            }

            // Another modeller build of the same schema has no exact profile and
            // resolves through the derived family; another schema resolves to nothing.
            let near = SchemaKey::parse(NEAR_SCHEMA_KEY);
            assert!(near.is_ok(), "{near:?}");
            if let Ok(near) = near {
                assert!(registry.profile_for_key(&near).is_none());
                let family = registry.provider_for_key(&near);
                assert!(family.is_some());
                if let Some(family) = family {
                    assert_eq!(family.profile().metadata().profile_id, "family-90000");
                    assert_eq!(
                        family.profile().metadata().coverage,
                        BuiltinProfileCoverage::FamilyInferred
                    );
                    assert_eq!(family.selected_schema_key(), NEAR_SCHEMA_KEY);
                    assert!(family.supports_schema_key(&near));
                }
            }
            let other_schema = SchemaKey::parse("SCH_1000000_90001");
            assert!(other_schema.is_ok(), "{other_schema:?}");
            if let Ok(other_schema) = other_schema {
                assert!(registry.provider_for_key(&other_schema).is_none());
            }
        }
    }

    #[test]
    fn compiled_registry_selects_only_exact_topsolid_keys() {
        let registry = BuiltinProfileRegistry::compiled();
        assert!(registry.is_ok(), "{registry:?}");
        let Ok(registry) = registry else {
            return;
        };

        let exact = SchemaKey::parse("SCH_3700192_36001_13006");
        assert!(exact.is_ok(), "{exact:?}");
        let Ok(exact) = exact else {
            return;
        };
        let provider = registry.provider_for_key(&exact);
        assert!(provider.is_some());
        let Some(provider) = provider else {
            return;
        };
        assert_eq!(provider.selected_schema_key(), exact.raw());
        assert!(provider.supports_schema_key(&exact));
        assert!(provider.contains_schema("13006"));
        assert!(provider.contains_schema("36001"));
        assert!(matches!(
            provider.provenance(),
            SchemaProviderProvenance::Builtin {
                profile_id: "topsolid-v6-psolid-36001-r1",
                schema_key: "SCH_3700192_36001_13006",
                ..
            }
        ));

        let unknown = SchemaKey::parse("SCH_3700193_36001_13006");
        assert!(unknown.is_ok(), "{unknown:?}");
        let Ok(unknown) = unknown else {
            return;
        };
        if let Some(provider) = registry.provider_for_key(&unknown) {
            assert!(matches!(
                provider.provenance(),
                SchemaProviderProvenance::Builtin {
                    profile_id,
                    coverage: BuiltinProfileCoverage::FamilyInferred,
                    ..
                } if !profile_id.starts_with("topsolid")
            ));
        }
    }

    /// Three builds of schema 90000: the first declares type 42, the second
    /// agrees on 42 and adds 43 and 44, the third declares another layout for 44.
    fn family_registry() -> Result<BuiltinProfileRegistry, ParseError> {
        let shared = profile_with("synthetic-sch90000-r1", SCHEMA_KEY, 42)?;
        let second = BuiltinSchemaProfile::new(
            metadata("synthetic-sch90000-build2-r1"),
            vec![NEAR_SCHEMA_KEY.to_owned()],
            vec![definition(42), definition(43), definition(44)],
        )?;
        let mut other_44 = definition(44);
        other_44.fields.push(FieldDefinition {
            name: "field_1".to_owned(),
            field_type: FieldType::Integer,
            pointer_class: 0,
            element_count: 0,
            transmitted: true,
        });
        let third = BuiltinSchemaProfile::new(
            metadata("synthetic-sch90000-build3-r1"),
            vec!["SCH_1000002_90000".to_owned()],
            vec![other_44],
        )?;
        BuiltinProfileRegistry::new(vec![shared, second, third])
    }

    #[test]
    fn family_merges_members_and_excludes_disagreements() {
        let registry = family_registry();
        assert!(registry.is_ok(), "{registry:?}");
        let Ok(registry) = registry else {
            return;
        };
        assert_eq!(registry.len(), 3);
        assert_eq!(registry.families().len(), 1);
        let family = registry.profile("family-90000");
        assert!(family.is_some());
        let Some(family) = family else {
            return;
        };
        assert_eq!(family.family_id(), Some("90000"));
        assert_eq!(family.metadata().revision, 1);
        assert_eq!(family.metadata().provider_schema, "90000");
        assert_eq!(
            family
                .members()
                .iter()
                .map(|member| member.profile_id.as_str())
                .collect::<Vec<_>>(),
            [
                "synthetic-sch90000-build2-r1",
                "synthetic-sch90000-build3-r1",
                "synthetic-sch90000-r1",
            ]
        );
        assert_eq!(family.definitions().len(), 2);
        assert!(family.definition(42).is_some() && family.definition(43).is_some());
        assert_eq!(family.conflicting_types().collect::<Vec<_>>(), [44]);
        assert_eq!(family.accepted_schema_keys().len(), 0);
        assert_eq!(family.metadata().profile_sha256.len(), 64);
        assert_eq!(
            family.metadata().profile_sha256,
            family_digest("90000", family.members(), family.base_members())
        );
        assert!(family.base_members().is_empty());
    }

    #[test]
    fn families_take_every_reviewed_declaration_of_their_provider_schema() {
        // A standard profile of schema 90000 and an embedded profile over base
        // 90000 describe the same provider schema: the standard family gains the
        // embedded profile's type 12 and the embedded family gains type 42.
        let standard = profile_with("synthetic-sch90000-r1", SCHEMA_KEY, 42);
        let mut embedded_metadata = metadata("synthetic-embedded-r1");
        embedded_metadata.provider_schema = "90000".to_owned();
        let embedded = BuiltinSchemaProfile::new_embedded(
            embedded_metadata,
            vec!["SCH_1000000_90001_90000".to_owned()],
            vec![definition(12)],
            vec![],
            vec![204],
        );
        assert!(standard.is_ok() && embedded.is_ok());
        let (Ok(standard), Ok(embedded)) = (standard, embedded) else {
            return;
        };
        let registry = BuiltinProfileRegistry::new(vec![standard, embedded]);
        assert!(registry.is_ok(), "{registry:?}");
        let Ok(registry) = registry else {
            return;
        };
        assert_eq!(registry.families().len(), 2);
        let standard_family = registry.profile("family-90000");
        let embedded_family = registry.profile("family-90001-90000");
        assert!(standard_family.is_some() && embedded_family.is_some());
        let (Some(standard_family), Some(embedded_family)) = (standard_family, embedded_family)
        else {
            return;
        };
        for family in [standard_family, embedded_family] {
            assert!(family.definition(12).is_some() && family.definition(42).is_some());
            assert_eq!(family.absent_base_types().collect::<Vec<_>>(), [204]);
            assert_eq!(family.members().len(), 1);
            assert_eq!(family.base_members().len(), 1);
        }
        assert_eq!(
            standard_family.base_members()[0].profile_id,
            "synthetic-embedded-r1"
        );
        assert_eq!(
            embedded_family.base_members()[0].profile_id,
            "synthetic-sch90000-r1"
        );
        assert_ne!(
            standard_family.metadata().profile_sha256,
            embedded_family.metadata().profile_sha256
        );
    }

    #[test]
    fn family_provider_parses_other_builds_and_fails_closed_on_disputed_types() {
        let registry = family_registry();
        assert!(registry.is_ok(), "{registry:?}");
        let Ok(registry) = registry else {
            return;
        };
        let unseen = SchemaKey::parse("SCH_1000003_90000");
        assert!(unseen.is_ok(), "{unseen:?}");
        let Ok(unseen) = unseen else {
            return;
        };
        let provider = registry.provider_for_key(&unseen);
        assert!(provider.is_some());
        let Some(provider) = provider else {
            return;
        };
        assert_eq!(provider.lookup_type("90000", 44), SchemaTypeLookup::Unknown);
        let parsed = parse_xt(
            &xt_payload("SCH_1000003_90000", 43, 9),
            &provider,
            DocumentLimits::default(),
        );
        assert!(parsed.is_ok(), "{parsed:?}");
        if let Ok(parsed) = parsed {
            assert!(matches!(
                &parsed.schema_provider,
                SchemaProviderResolution::Builtin {
                    profile_id,
                    profile_revision: 1,
                    schema_key,
                    coverage: BuiltinProfileCoverage::FamilyInferred,
                    ..
                } if profile_id == "family-90000" && schema_key == "SCH_1000003_90000"
            ));
        }
        let conflicting = parse_xb(
            &xb_payload("SCH_1000003_90000", 44, 9),
            &provider,
            DocumentLimits::default(),
        );
        assert_eq!(
            conflicting.as_ref().err().map(ParseError::kind),
            Some(ErrorKind::BuiltinProfileUncoveredType)
        );
        // The family provider is pinned to the key it was selected for.
        let exact = SchemaKey::parse(SCHEMA_KEY);
        if let Ok(exact) = exact {
            assert!(!provider.supports_schema_key(&exact));
        }
    }

    #[test]
    fn embedded_family_merges_membership_and_rejects_contradictions() {
        let embedded = |id: &str, key: &str, types: Vec<u16>, absent: Vec<u16>| {
            let mut meta = metadata(id);
            meta.provider_schema = "13006".to_owned();
            BuiltinSchemaProfile::new_embedded(
                meta,
                vec![key.to_owned()],
                types.into_iter().map(definition).collect(),
                vec![],
                absent,
            )
        };
        let first = embedded(
            "synthetic-a-r1",
            "SCH_1000000_90000_13006",
            vec![12],
            vec![204],
        );
        // Type 45 is declared here but recorded absent by the other member.
        let second = embedded(
            "synthetic-b-r1",
            "SCH_1000001_90000_13006",
            vec![12, 45],
            vec![176],
        );
        let third = embedded(
            "synthetic-c-r1",
            "SCH_1000002_90000_13006",
            vec![12],
            vec![45],
        );
        assert!(first.is_ok() && second.is_ok() && third.is_ok());
        let (Ok(first), Ok(second), Ok(third)) = (first, second, third) else {
            return;
        };
        let registry = BuiltinProfileRegistry::new(vec![first, second, third]);
        assert!(registry.is_ok(), "{registry:?}");
        let Ok(registry) = registry else {
            return;
        };
        let key = SchemaKey::parse("SCH_1000009_90000_13006");
        assert!(key.is_ok(), "{key:?}");
        let Ok(key) = key else {
            return;
        };
        let provider = registry.provider_for_key(&key);
        assert!(provider.is_some());
        let Some(provider) = provider else {
            return;
        };
        let family = provider.profile();
        assert_eq!(family.metadata().profile_id, "family-90000-13006");
        assert_eq!(family.metadata().provider_schema, "13006");
        assert_eq!(family.absent_base_types().collect::<Vec<_>>(), [176, 204]);
        assert_eq!(family.conflicting_types().collect::<Vec<_>>(), [45]);
        assert!(matches!(
            provider.lookup_type("13006", 12),
            SchemaTypeLookup::Defined(_)
        ));
        assert_eq!(provider.lookup_type("13006", 204), SchemaTypeLookup::Absent);
        assert_eq!(provider.lookup_type("13006", 45), SchemaTypeLookup::Unknown);
        // A standard key of the same effective schema belongs to another family.
        let standard = SchemaKey::parse("SCH_1000009_90000");
        if let Ok(standard) = standard {
            assert!(registry.provider_for_key(&standard).is_none());
        }
    }

    #[test]
    fn family_profiles_cannot_be_inserted_or_nested() {
        let registry = registry();
        assert!(registry.is_ok(), "{registry:?}");
        let Ok(mut registry) = registry else {
            return;
        };
        let family = registry.profile("family-90000").cloned();
        assert!(family.is_some());
        let Some(family) = family else {
            return;
        };
        assert_eq!(
            registry
                .insert(family.clone())
                .err()
                .map(|error| error.kind()),
            Some(ErrorKind::InvalidBuiltinProfile)
        );
        assert_eq!(
            BuiltinSchemaProfile::new_family("90000", &[&family])
                .err()
                .map(|error| error.kind()),
            Some(ErrorKind::InvalidBuiltinProfile)
        );
        assert_eq!(
            BuiltinSchemaProfile::new_family("90000", &[])
                .err()
                .map(|error| error.kind()),
            Some(ErrorKind::InvalidBuiltinProfile)
        );
    }

    #[test]
    fn parses_synthetic_text_and_binary_with_one_builtin_profile() {
        let registry = registry();
        assert!(registry.is_ok(), "{registry:?}");
        let schema_key = SchemaKey::parse(SCHEMA_KEY);
        assert!(schema_key.is_ok(), "{schema_key:?}");
        if let (Ok(registry), Ok(schema_key)) = (registry, schema_key) {
            let provider = registry.provider_for_key(&schema_key);
            assert!(provider.is_some());
            if let Some(provider) = provider {
                let text_bytes = xt_payload(SCHEMA_KEY, 42, 7);
                let binary_bytes = xb_payload(SCHEMA_KEY, 42, 7);
                let text = parse_xt(&text_bytes, &provider, DocumentLimits::default());
                let binary = parse_xb(&binary_bytes, &provider, DocumentLimits::default());
                assert!(text.is_ok(), "{text:?}");
                assert!(binary.is_ok(), "{binary:?}");
                if let (Ok(text), Ok(binary)) = (text, binary) {
                    assert!(matches!(
                        &text.schema_provider,
                        SchemaProviderResolution::Builtin {
                            profile_id,
                            profile_revision: 1,
                            schema_key,
                            coverage: BuiltinProfileCoverage::VerifiedSubset,
                            profile_sha256,
                        } if profile_id == "synthetic-sch90000-r1"
                            && schema_key == SCHEMA_KEY
                            && profile_sha256 == &"0".repeat(64)
                    ));
                    assert_eq!(text.schema_provider, binary.schema_provider);
                    assert_eq!(text.nodes.len(), 1);
                    assert_eq!(binary.nodes.len(), 1);
                    assert_eq!(text.nodes[0].node_type, binary.nodes[0].node_type);
                    assert_eq!(text.nodes[0].index, binary.nodes[0].index);
                    assert_eq!(
                        text.nodes[0].fields[0].values,
                        [FieldValue::Integer(Some(7))]
                    );
                    assert_eq!(
                        binary.nodes[0].fields[0].values,
                        [FieldValue::Integer(Some(7))]
                    );
                    assert_eq!(write_xb(&binary), binary_bytes);
                }
            }
        }
    }

    #[test]
    fn rejects_near_keys_and_uncovered_types_without_guessing() {
        let registry = registry();
        let schema_key = SchemaKey::parse(SCHEMA_KEY);
        assert!(registry.is_ok(), "{registry:?}");
        assert!(schema_key.is_ok(), "{schema_key:?}");
        if let (Ok(registry), Ok(schema_key)) = (registry, schema_key) {
            let provider = registry.provider_for_key(&schema_key);
            assert!(provider.is_some());
            if let Some(provider) = provider {
                for near_error in [
                    parse_xt(
                        &xt_payload(NEAR_SCHEMA_KEY, 42, 7),
                        &provider,
                        DocumentLimits::default(),
                    )
                    .err(),
                    parse_xb(
                        &xb_payload(NEAR_SCHEMA_KEY, 42, 7),
                        &provider,
                        DocumentLimits::default(),
                    )
                    .err(),
                ] {
                    assert_eq!(
                        near_error.as_ref().map(ParseError::kind),
                        Some(ErrorKind::UnsupportedBuiltinSchemaKey)
                    );
                    assert!(matches!(
                        near_error.as_ref().map(ParseError::details),
                        Some(ErrorDetails::BuiltinProfileLookup {
                            profile_id,
                            profile_revision: 1,
                            selected_schema_key,
                            requested_schema_key,
                            node_type: 0,
                        }) if profile_id == "synthetic-sch90000-r1"
                            && selected_schema_key == SCHEMA_KEY
                            && requested_schema_key == NEAR_SCHEMA_KEY
                    ));
                }

                for error in [
                    parse_xt(
                        &xt_payload(SCHEMA_KEY, 43, 7),
                        &provider,
                        DocumentLimits::default(),
                    )
                    .err(),
                    parse_xb(
                        &xb_payload(SCHEMA_KEY, 43, 7),
                        &provider,
                        DocumentLimits::default(),
                    )
                    .err(),
                ] {
                    assert_eq!(
                        error.as_ref().map(ParseError::kind),
                        Some(ErrorKind::BuiltinProfileUncoveredType)
                    );
                    assert!(matches!(
                        error.as_ref().map(ParseError::details),
                        Some(ErrorDetails::BuiltinProfileLookup {
                            profile_id,
                            profile_revision: 1,
                            selected_schema_key,
                            requested_schema_key,
                            node_type: 43,
                        }) if profile_id == "synthetic-sch90000-r1"
                            && selected_schema_key == SCHEMA_KEY
                            && requested_schema_key == SCHEMA_KEY
                    ));
                }
            }
        }
    }

    #[test]
    fn rejects_invalid_profiles_and_registry_collisions() {
        let mut invalid_metadata = metadata("synthetic-sch90000-r1");
        invalid_metadata.profile_sha256 = "not-a-digest".to_owned();
        let invalid_digest = BuiltinSchemaProfile::new(
            invalid_metadata,
            vec![SCHEMA_KEY.to_owned()],
            vec![definition(42)],
        );
        assert_eq!(
            invalid_digest.as_ref().err().map(ParseError::kind),
            Some(ErrorKind::InvalidBuiltinProfile)
        );

        let embedded_key = BuiltinSchemaProfile::new(
            metadata("synthetic-sch90000-r1"),
            vec!["SCH_1000000_90001_90000".to_owned()],
            vec![definition(42)],
        );
        assert_eq!(
            embedded_key.as_ref().err().map(ParseError::kind),
            Some(ErrorKind::InvalidBuiltinProfile)
        );

        let duplicate_type = BuiltinSchemaProfile::new(
            metadata("synthetic-sch90000-r1"),
            vec![SCHEMA_KEY.to_owned()],
            vec![definition(42), definition(42)],
        );
        assert_eq!(
            duplicate_type.as_ref().err().map(ParseError::kind),
            Some(ErrorKind::InvalidBuiltinProfile)
        );

        let mut repeated_field = definition(42);
        repeated_field.fields.push(repeated_field.fields[0].clone());
        let duplicate_field = BuiltinSchemaProfile::new(
            metadata("synthetic-sch90000-r1"),
            vec![SCHEMA_KEY.to_owned()],
            vec![repeated_field],
        );
        assert_eq!(
            duplicate_field.as_ref().err().map(ParseError::kind),
            Some(ErrorKind::InvalidBuiltinProfile)
        );

        let registry = registry();
        assert!(registry.is_ok(), "{registry:?}");
        if let Ok(mut registry) = registry {
            let duplicate_id = profile_with("synthetic-sch90000-r1", NEAR_SCHEMA_KEY, 43);
            assert!(duplicate_id.is_ok(), "{duplicate_id:?}");
            if let Ok(duplicate_id) = duplicate_id {
                assert_eq!(
                    registry
                        .insert(duplicate_id)
                        .err()
                        .map(|error| error.kind()),
                    Some(ErrorKind::InvalidBuiltinProfile)
                );
            }

            let duplicate_key = profile_with("synthetic-sch90000-r2", SCHEMA_KEY, 43);
            assert!(duplicate_key.is_ok(), "{duplicate_key:?}");
            if let Ok(duplicate_key) = duplicate_key {
                assert_eq!(
                    registry
                        .insert(duplicate_key)
                        .err()
                        .map(|error| error.kind()),
                    Some(ErrorKind::InvalidBuiltinProfile)
                );
            }
            assert_eq!(registry.len(), 1);
        }
    }
}
