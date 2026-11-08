"""Author-created named-catalog streams; no vendor files or definitions are copied."""

import struct

from parasolid_kit import InMemorySchemaProvider, SchemaCatalog, parse_xb, parse_xt
from parasolid_kit.schema import FieldDefinition, FieldType, SchemaSource, TypeDefinition
from tests.support.parasolid_binary import SyntheticXbBuilder
from tests.support.parasolid_schema import positive_integer
from tests.support.parasolid_text import text_header


def stored_document(
    encoding="x_t",
    *,
    indices=(0, 1, 2),
    position_count=3,
    embedded=True,
    normal_type=1,
    capacity=4,
    sense=43,
    transform=False,
):
    def f(name, kind, pointer=0, count=0):
        return FieldDefinition(name, FieldType(kind), pointer, count, True)

    definitions = (
        (180, "POLYLINE", (f("sense", "c"), f("data", "p", 181))),
        (
            181,
            "POLYLINE_DATA",
            (f("n_pvecs", "d"), f("closed", "l"), f("base_parm", "f"), f("pvec", "p", 85)),
        ),
        (85, "POINT_VALUES", (f("values", "v", count=1),)),
        (118, "MESH", (f("sense", "c"), f("transform", "p", 100), f("psm_imesh", "p", 189))),
        (
            189,
            "PSM_MESH",
            (
                f("owner", "p", 0),
                f("precision", "u"),
                f("normal_type", "u"),
                f("position_pool", "p", 193),
                f("position_indices", "p", 191),
                f("normal_pool", "p", 193),
                f("normal_indices", "p", 191),
            ),
        ),
        (
            193,
            "VECTOR_COMB",
            (
                f("n_vectors", "d"),
                f("n_max_vectors", "d"),
                f("shift", "d"),
                f("encoding", "u"),
                f("teeth", "p", 192, 1),
            ),
        ),
        (192, "VECTOR_TOOTH", (f("values", "f", count=1),)),
        (
            191,
            "INTEGER_COMB",
            (
                f("n_integers", "d"),
                f("n_max_integers", "d"),
                f("shift", "d"),
                f("encoding", "u"),
                f("n_bits_per_integer", "d"),
                f("teeth", "p", 190, 1),
            ),
        ),
        (190, "INTEGER_TOOTH", (f("values", "d", count=1),)),
        (
            100,
            "TRANSFORM",
            (
                f("rotation_matrix", "f", count=9),
                f("translation_vector", "v"),
                f("scale", "f"),
                f("flag", "d"),
                f("perspective_vector", "v"),
            ),
        ),
    )
    catalog = SchemaCatalog(
        "99001",
        tuple(
            TypeDefinition(
                number,
                name,
                name,
                any(field.element_count == 1 for field in fields),
                fields,
                SchemaSource.BASE,
            )
            for number, name, fields in definitions
        ),
    )
    provider = InMemorySchemaProvider((catalog,))
    records = [
        (180, 1, (sense, 2)),
        (181, 2, (3, True, 17.0, 3)),
        (85, 3, (((0.0, 0.0, 1.0), (2.0, 0.0, 1.0), (2.0, 3.0, 1.0)),)),
        (118, 4, (ord("+"), 10 if transform else 0, 5 if embedded else 0)),
        (189, 5, (0, 1, normal_type, 6, 8, 0, 0)),
        (193, 6, (position_count, capacity, 2, 1, (7,))),
        (192, 7, ((0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0, 90.0, 90.0, 90.0),)),
        (191, 8, (3, 4, 2, 1, 32, (9,))),
        (190, 9, ((*indices, 99),)),
    ]
    if transform:
        records.append(
            (
                100,
                10,
                (
                    (0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0),
                    (10.0, 0.0, 0.0),
                    2.0,
                    3,
                    (0.0, 0.0, 0.0),
                ),
            )
        )
    by_type = {d.node_type: d for d in catalog.definitions}

    def scalar(kind, value):
        if encoding == "x_t":
            if kind is FieldType.CHARACTER:
                return chr(value).encode()
            if kind is FieldType.LOGICAL:
                return b"T" if value else b"F"
            if kind is FieldType.VECTOR:
                return "".join(f"{v} " for v in value).encode()
            return f"{value} ".encode()
        if kind is FieldType.POINTER_INDEX:
            return positive_integer(value)
        if kind in (FieldType.CHARACTER, FieldType.UNSIGNED_BYTE, FieldType.LOGICAL):
            return bytes([int(value)])
        if kind is FieldType.VECTOR:
            return struct.pack(">3d", *value)
        return struct.pack(">d" if kind is FieldType.DOUBLE else ">i", value)

    binary = SyntheticXbBuilder(schema_name="SCH_3900000_99001", schema_max_type=None)
    text = text_header("SCH_3900000_99001")
    for number, index, values in records:
        definition = by_type[number]
        variable_count = next(
            (
                len(value)
                for field, value in zip(definition.fields, values, strict=True)
                if field.element_count == 1
            ),
            None,
        )
        payload = b""
        if variable_count is not None:
            payload += (
                f"{variable_count} ".encode()
                if encoding == "x_t"
                else struct.pack(">i", variable_count)
            )
        payload += f"{index} ".encode() if encoding == "x_t" else positive_integer(index)
        for field, value in zip(definition.fields, values, strict=True):
            parts = value if field.element_count else (value,)
            payload += b"".join(scalar(field.field_type, part) for part in parts)
        if encoding == "x_t":
            text += f"{number} ".encode() + payload
        else:
            binary.add_raw_node(number, payload)
    return (
        parse_xt(text + b"1 0 ", schema_provider=provider)
        if encoding == "x_t"
        else parse_xb(binary.build(), schema_provider=provider)
    )
