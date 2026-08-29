"""Small deterministic GLB 2.0 writer and structural validator."""

from __future__ import annotations

import json
import struct
from dataclasses import dataclass, field
from math import isfinite
from typing import Final

from .model import GlbValidationReport

_GLB_MAGIC: Final = b"glTF"
_JSON_CHUNK: Final = 0x4E4F534A
_BIN_CHUNK: Final = 0x004E4942
_ARRAY_BUFFER: Final = 34962
_ELEMENT_ARRAY_BUFFER: Final = 34963
_FLOAT: Final = 5126
_UNSIGNED_INT: Final = 5125


@dataclass(slots=True)
class GlbBuilder:
    """Append one deterministic primitive at a time without external libraries."""

    max_binary_bytes: int
    binary: bytearray = field(init=False, default_factory=bytearray, repr=False)
    buffer_views: list[dict[str, object]] = field(init=False, default_factory=list, repr=False)
    accessors: list[dict[str, object]] = field(init=False, default_factory=list, repr=False)
    primitives: list[dict[str, object]] = field(init=False, default_factory=list, repr=False)

    def __post_init__(self) -> None:
        if isinstance(self.max_binary_bytes, bool) or not isinstance(self.max_binary_bytes, int):
            raise ValueError("max_binary_bytes must be a positive integer")
        if self.max_binary_bytes <= 0:
            raise ValueError("max_binary_bytes must be a positive integer")

    def add_triangles(
        self,
        *,
        target_key: str,
        pick_id: int,
        positions: list[float],
        normals: list[float],
        indices: list[int],
    ) -> int:
        if len(positions) % 3 or len(normals) != len(positions) or len(indices) % 3:
            raise ValueError("triangle primitive arrays have inconsistent lengths")
        position_accessor = self._add_float_vectors(positions, target=_ARRAY_BUFFER)
        normal_accessor = self._add_float_vectors(normals, target=_ARRAY_BUFFER, bounds=False)
        index_accessor = self._add_indices(indices)
        primitive_index = len(self.primitives)
        self.primitives.append(
            {
                "attributes": {"NORMAL": normal_accessor, "POSITION": position_accessor},
                "indices": index_accessor,
                "material": 0,
                "mode": 4,
                "extras": {
                    "entityKind": "face",
                    "pickId": pick_id,
                    "targetKey": target_key,
                },
            }
        )
        return primitive_index

    def add_line_strip(
        self,
        *,
        target_key: str,
        pick_id: int,
        positions: list[float],
    ) -> int:
        if len(positions) % 3 or len(positions) < 6:
            raise ValueError("line primitive must contain at least two 3D points")
        position_accessor = self._add_float_vectors(positions, target=_ARRAY_BUFFER)
        indices = list(range(len(positions) // 3))
        index_accessor = self._add_indices(indices)
        primitive_index = len(self.primitives)
        self.primitives.append(
            {
                "attributes": {"POSITION": position_accessor},
                "indices": index_accessor,
                "material": 1,
                "mode": 3,
                "extras": {
                    "entityKind": "edge",
                    "pickId": pick_id,
                    "targetKey": target_key,
                },
            }
        )
        return primitive_index

    def build(self) -> bytes:
        document: dict[str, object] = {
            "asset": {
                "generator": "parasolid-kit preview",
                "version": "2.0",
            },
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0, "name": "Parasolid preview"}],
            "meshes": [{"name": "Converted B-Rep", "primitives": self.primitives}],
            "materials": [
                {
                    "name": "Faces",
                    "pbrMetallicRoughness": {
                        "baseColorFactor": [0.18, 0.52, 0.82, 1.0],
                        "metallicFactor": 0.0,
                        "roughnessFactor": 0.72,
                    },
                },
                {
                    "name": "Edges",
                    "pbrMetallicRoughness": {
                        "baseColorFactor": [0.04, 0.08, 0.12, 1.0],
                        "metallicFactor": 0.0,
                        "roughnessFactor": 1.0,
                    },
                },
            ],
            "buffers": [{"byteLength": len(self.binary)}],
            "bufferViews": self.buffer_views,
            "accessors": self.accessors,
        }
        json_bytes = json.dumps(
            document,
            ensure_ascii=True,
            separators=(",", ":"),
            sort_keys=True,
        ).encode("ascii")
        json_bytes += b" " * (-len(json_bytes) % 4)
        binary = bytes(self.binary)
        binary += b"\0" * (-len(binary) % 4)
        total_length = 12 + 8 + len(json_bytes) + 8 + len(binary)
        return b"".join(
            (
                struct.pack("<4sII", _GLB_MAGIC, 2, total_length),
                struct.pack("<II", len(json_bytes), _JSON_CHUNK),
                json_bytes,
                struct.pack("<II", len(binary), _BIN_CHUNK),
                binary,
            )
        )

    def _add_float_vectors(
        self,
        values: list[float],
        *,
        target: int,
        bounds: bool = True,
    ) -> int:
        if not values or len(values) % 3:
            raise ValueError("vector accessor must contain complete 3D values")
        if any(not isfinite(value) for value in values):
            raise ValueError("vector accessor contains a non-finite value")
        payload = struct.pack(f"<{len(values)}f", *values)
        view = self._add_buffer_view(payload, target=target)
        accessor: dict[str, object] = {
            "bufferView": view,
            "componentType": _FLOAT,
            "count": len(values) // 3,
            "type": "VEC3",
        }
        if bounds:
            axes = tuple(values[index::3] for index in range(3))
            accessor["min"] = [min(axis) for axis in axes]
            accessor["max"] = [max(axis) for axis in axes]
        index = len(self.accessors)
        self.accessors.append(accessor)
        return index

    def _add_indices(self, values: list[int]) -> int:
        if not values or any(value < 0 or value > 0xFFFFFFFF for value in values):
            raise ValueError("index accessor contains no values or an out-of-range index")
        payload = struct.pack(f"<{len(values)}I", *values)
        view = self._add_buffer_view(payload, target=_ELEMENT_ARRAY_BUFFER)
        index = len(self.accessors)
        self.accessors.append(
            {
                "bufferView": view,
                "componentType": _UNSIGNED_INT,
                "count": len(values),
                "max": [max(values)],
                "min": [min(values)],
                "type": "SCALAR",
            }
        )
        return index

    def _add_buffer_view(self, payload: bytes, *, target: int) -> int:
        padding = -len(self.binary) % 4
        projected = len(self.binary) + padding + len(payload)
        if projected > self.max_binary_bytes:
            raise OverflowError("GLB binary buffer exceeds max_output_bytes")
        if padding:
            self.binary.extend(b"\0" * padding)
        offset = len(self.binary)
        self.binary.extend(payload)
        index = len(self.buffer_views)
        self.buffer_views.append(
            {
                "buffer": 0,
                "byteLength": len(payload),
                "byteOffset": offset,
                "target": target,
            }
        )
        return index


def validate_glb_bytes(payload: bytes) -> GlbValidationReport:
    """Validate the self-contained GLB structure used by the preview UI."""

    errors: list[str] = []
    version = 0
    declared_length = 0
    binary_length = 0
    mesh_count = 0
    primitive_count = 0
    accessor_count = 0
    document: dict[str, object] = {}
    binary = b""
    if len(payload) < 20:
        errors.append("GLB is shorter than its header and first chunk")
    else:
        magic, version, declared_length = struct.unpack_from("<4sII", payload, 0)
        if magic != _GLB_MAGIC:
            errors.append("GLB magic is invalid")
        if version != 2:
            errors.append("GLB version must be 2")
        if declared_length != len(payload):
            errors.append("GLB declared length differs from its byte length")
        offset = 12
        chunks: list[tuple[int, bytes]] = []
        while offset + 8 <= len(payload):
            length, chunk_type = struct.unpack_from("<II", payload, offset)
            offset += 8
            if length % 4:
                errors.append("GLB chunk length must be four-byte aligned")
            end = offset + length
            if end > len(payload):
                errors.append("GLB chunk extends past the declared payload")
                break
            chunks.append((chunk_type, payload[offset:end]))
            offset = end
        if offset != len(payload):
            errors.append("GLB contains trailing or truncated chunk bytes")
        if not chunks or chunks[0][0] != _JSON_CHUNK:
            errors.append("GLB first chunk must be JSON")
        if len(chunks) != 2 or chunks[-1][0] != _BIN_CHUNK:
            errors.append("preview GLB must contain exactly one JSON and one BIN chunk")
        if chunks and chunks[0][0] == _JSON_CHUNK:
            try:
                decoded = json.loads(chunks[0][1].decode("utf-8"))
                if isinstance(decoded, dict):
                    document = decoded
                else:
                    errors.append("GLB JSON root must be an object")
            except (UnicodeDecodeError, json.JSONDecodeError) as error:
                errors.append(f"GLB JSON is invalid: {error}")
        if len(chunks) == 2 and chunks[1][0] == _BIN_CHUNK:
            binary = chunks[1][1]
            binary_length = len(binary)

    if document:
        asset = document.get("asset")
        if not isinstance(asset, dict) or asset.get("version") != "2.0":
            errors.append("GLB asset.version must be 2.0")
        buffers = document.get("buffers")
        if not isinstance(buffers, list) or len(buffers) != 1 or not isinstance(buffers[0], dict):
            errors.append("preview GLB must contain one embedded buffer")
        else:
            byte_length = buffers[0].get("byteLength")
            if not isinstance(byte_length, int) or byte_length < 0 or byte_length > len(binary):
                errors.append("GLB buffer byteLength exceeds the BIN chunk")
            if "uri" in buffers[0]:
                errors.append("preview GLB buffer must not use an external URI")
        views = document.get("bufferViews")
        accessors = document.get("accessors")
        meshes = document.get("meshes")
        if not isinstance(views, list):
            views = []
            errors.append("GLB bufferViews must be an array")
        for view in views:
            if not isinstance(view, dict):
                errors.append("GLB bufferView must be an object")
                continue
            offset = view.get("byteOffset", 0)
            length = view.get("byteLength")
            if (
                not isinstance(offset, int)
                or not isinstance(length, int)
                or offset < 0
                or length < 0
                or offset + length > len(binary)
            ):
                errors.append("GLB bufferView exceeds the BIN chunk")
        if not isinstance(accessors, list):
            accessors = []
            errors.append("GLB accessors must be an array")
        accessor_count = len(accessors)
        component_sizes = {_FLOAT: 4, _UNSIGNED_INT: 4}
        component_counts = {"SCALAR": 1, "VEC3": 3}
        for accessor in accessors:
            if not isinstance(accessor, dict):
                errors.append("GLB accessor must be an object")
                continue
            view_index = accessor.get("bufferView")
            count = accessor.get("count")
            if not isinstance(view_index, int) or not 0 <= view_index < len(views):
                errors.append("GLB accessor references an unknown bufferView")
            if not isinstance(count, int) or count <= 0:
                errors.append("GLB accessor count must be positive")
            component_size = component_sizes.get(accessor.get("componentType"))
            component_count = component_counts.get(accessor.get("type"))
            accessor_offset = accessor.get("byteOffset", 0)
            if component_size is None or component_count is None:
                errors.append("GLB accessor uses an unsupported component/type combination")
            elif not isinstance(accessor_offset, int) or accessor_offset < 0:
                errors.append("GLB accessor byteOffset must be non-negative")
            elif (
                isinstance(view_index, int)
                and 0 <= view_index < len(views)
                and isinstance(views[view_index], dict)
                and isinstance(count, int)
                and count > 0
            ):
                view_length = views[view_index].get("byteLength")
                required = accessor_offset + count * component_size * component_count
                if not isinstance(view_length, int) or required > view_length:
                    errors.append("GLB accessor exceeds its bufferView")
        if not isinstance(meshes, list):
            meshes = []
            errors.append("GLB meshes must be an array")
        mesh_count = len(meshes)
        if mesh_count != 1:
            errors.append("preview GLB must contain exactly one mesh")
        pick_ids: set[int] = set()
        for mesh in meshes:
            primitives = mesh.get("primitives") if isinstance(mesh, dict) else None
            if not isinstance(primitives, list):
                errors.append("GLB mesh primitives must be an array")
                continue
            primitive_count += len(primitives)
            for primitive in primitives:
                if not isinstance(primitive, dict) or primitive.get("mode") not in {3, 4}:
                    errors.append("preview GLB primitive mode must be LINE_STRIP or TRIANGLES")
                    continue
                mode = primitive["mode"]
                attributes = primitive.get("attributes")
                position = attributes.get("POSITION") if isinstance(attributes, dict) else None
                normal = attributes.get("NORMAL") if isinstance(attributes, dict) else None
                indices = primitive.get("indices")
                if not isinstance(position, int) or not 0 <= position < len(accessors):
                    errors.append("preview GLB primitive must reference a POSITION accessor")
                if not isinstance(indices, int) or not 0 <= indices < len(accessors):
                    errors.append("preview GLB primitive must reference an index accessor")
                if mode == 4 and (not isinstance(normal, int) or not 0 <= normal < len(accessors)):
                    errors.append("preview triangle primitive must reference a NORMAL accessor")
                extras = primitive.get("extras")
                pick_id = extras.get("pickId") if isinstance(extras, dict) else None
                entity_kind = extras.get("entityKind") if isinstance(extras, dict) else None
                target_key = extras.get("targetKey") if isinstance(extras, dict) else None
                expected_kind = "face" if mode == 4 else "edge"
                if entity_kind != expected_kind:
                    errors.append("preview GLB primitive entityKind differs from its mode")
                if not isinstance(target_key, str) or not target_key.startswith(
                    f"occt:{expected_kind}:"
                ):
                    errors.append("preview GLB primitive has an invalid targetKey")
                if not isinstance(pick_id, int) or not 1 <= pick_id <= 0xFFFFFF:
                    errors.append("preview GLB primitive has an invalid pickId")
                elif pick_id in pick_ids:
                    errors.append("preview GLB primitive pickId values must be unique")
                else:
                    pick_ids.add(pick_id)
        if primitive_count == 0:
            errors.append("preview GLB contains no primitives")
        for external_key in ("images", "textures"):
            if document.get(external_key):
                errors.append(f"preview GLB must not contain {external_key}")

    return GlbValidationReport(
        valid=not errors,
        version=version,
        declared_length=declared_length,
        binary_length=binary_length,
        mesh_count=mesh_count,
        primitive_count=primitive_count,
        accessor_count=accessor_count,
        errors=tuple(errors),
    )


__all__ = ["GlbBuilder", "validate_glb_bytes"]
