"""TITLEPIC extraction from WAD files + Doom patch format decoder.

Doom WAD format:
- Header: 4 bytes magic ("IWAD"/"PWAD"), 4 bytes lump count (LE int32), 4 bytes dir offset (LE int32)
- Directory: 16 bytes per entry — offset (4), size (4), name (8, null-padded ASCII)
- TITLEPIC lump: either Doom column-based patch format or raw PNG

Doom patch format (320x200):
- 8-byte header: width (2), height (2), left_offset (2), top_offset (2)
- Column offsets: width * 4 bytes (one uint32 per column)
- Each column: sequence of posts (spans)
  - Post: top_delta (1 byte), length (1 byte), padding (1 byte), pixel data (length bytes), padding (1 byte)
  - Column ends with top_delta == 0xFF
"""

import mmap
import struct
import zipfile
from io import BytesIO
from pathlib import Path

# Maximum size for a WAD inside a ZIP (256 MB) — protects against decompression bombs
_MAX_ZIP_ENTRY_SIZE = 256 * 1024 * 1024

try:
    from PIL import Image
except ImportError:
    Image = None

# Standard Doom PLAYPAL (first palette entry, 768 bytes = 256 * RGB).
# This is public domain data from the Doom engine source release.
# We embed a minimal version: the standard palette is always available
# from the WAD itself, but as a fallback we use a well-known approximation.
_DOOM_PALETTE = None  # Loaded lazily from WAD or embedded fallback


def _read_palette_from_wad(wad_data: bytes) -> bytes | None:
    """Try to read PLAYPAL lump from WAD data."""
    try:
        directory = _parse_wad_directory(wad_data)
        for name, offset, size in directory:
            if name == "PLAYPAL" and size >= 768:
                return wad_data[offset:offset + 768]
    except Exception:
        pass
    return None


def _parse_wad_directory(wad_data: bytes) -> list[tuple[str, int, int]]:
    """Parse WAD header and directory. Returns [(name, offset, size), ...]."""
    if len(wad_data) < 12:
        return []

    magic = wad_data[:4]
    if magic not in (b"IWAD", b"PWAD"):
        return []

    num_lumps = struct.unpack_from("<i", wad_data, 4)[0]
    dir_offset = struct.unpack_from("<i", wad_data, 8)[0]

    entries = []
    for i in range(num_lumps):
        entry_offset = dir_offset + i * 16
        if entry_offset + 16 > len(wad_data):
            break

        lump_offset = struct.unpack_from("<i", wad_data, entry_offset)[0]
        lump_size = struct.unpack_from("<i", wad_data, entry_offset + 4)[0]
        name_bytes = wad_data[entry_offset + 8:entry_offset + 16]
        name = name_bytes.split(b"\x00")[0].decode("ascii", errors="replace").upper()
        entries.append((name, lump_offset, lump_size))

    return entries


def _decode_doom_patch(data: bytes, palette: bytes, width: int = 320, height: int = 200) -> Image.Image | None:
    """Decode Doom column-based patch format into a PIL Image."""
    if Image is None:
        return None

    if len(data) < 8:
        return None

    img_width = struct.unpack_from("<H", data, 0)[0]
    img_height = struct.unpack_from("<H", data, 2)[0]

    # Sanity check dimensions
    if img_width < 1 or img_width > 2048 or img_height < 1 or img_height > 2048:
        return None

    # Need column offsets
    if len(data) < 8 + img_width * 4:
        return None

    img = Image.new("RGB", (img_width, img_height), (0, 0, 0))
    pixels = img.load()

    for col in range(img_width):
        col_offset = struct.unpack_from("<I", data, 8 + col * 4)[0]

        if col_offset >= len(data):
            continue

        pos = col_offset
        while pos < len(data):
            top_delta = data[pos]
            if top_delta == 0xFF:
                break

            if pos + 1 >= len(data):
                break
            length = data[pos + 1]

            # Skip padding byte, read pixel data, skip trailing padding
            pixel_start = pos + 3
            if pixel_start + length > len(data):
                break

            for y in range(length):
                row = top_delta + y
                if row < img_height:
                    palette_idx = data[pixel_start + y]
                    if palette_idx * 3 + 2 < len(palette):
                        r = palette[palette_idx * 3]
                        g = palette[palette_idx * 3 + 1]
                        b = palette[palette_idx * 3 + 2]
                        pixels[col, row] = (r, g, b)

            pos += length + 4  # top_delta + length + padding + data + padding

    return img


def extract_titlepic(wad_path: str | Path) -> Image.Image | None:
    """Extract TITLEPIC from a WAD file and return as PIL Image.

    Handles:
    - Direct .wad files
    - ZIP-wrapped WADs (finds the .wad inside)
    - PNG-encoded TITLEPICs (modern source ports)
    - Doom column-based patch format TITLEPICs
    """
    if Image is None:
        return None

    path = Path(wad_path)
    if not path.exists():
        return None

    wad_data = None

    # Check if it's a ZIP file containing a WAD
    if path.suffix.lower() == ".zip" or (path.suffix.lower() not in (".wad", ".pk3", ".pk7")):
        try:
            with zipfile.ZipFile(path) as zf:
                for info in zf.infolist():
                    if info.filename.lower().endswith(".wad"):
                        if info.file_size > _MAX_ZIP_ENTRY_SIZE:
                            break  # Skip oversized entries
                        wad_data = zf.read(info)
                        break
        except (zipfile.BadZipFile, KeyError):
            pass

    mm = None
    fh = None
    try:
        if wad_data is None:
            try:
                # Use mmap for direct WAD files to avoid loading entire file into memory
                fh = open(path, "rb")
                mm = mmap.mmap(fh.fileno(), 0, access=mmap.ACCESS_READ)
                wad_data = mm
            except (OSError, ValueError):
                return None

        # Parse WAD directory
        directory = _parse_wad_directory(wad_data)
        if not directory:
            return None

        # Find TITLEPIC lump
        titlepic_data = None
        for name, offset, size in directory:
            if name == "TITLEPIC" and size > 0:
                titlepic_data = bytes(wad_data[offset:offset + size])
                break

        if not titlepic_data:
            return None

        # Check if it's a PNG (modern WADs)
        if titlepic_data[:4] == b"\x89PNG":
            try:
                return Image.open(BytesIO(titlepic_data))
            except Exception:
                return None

        # Try Doom patch format — extract palette before closing mmap
        palette = _read_palette_from_wad(wad_data)
        if not palette:
            # Use a basic grayscale fallback palette
            palette = bytes(val for val in range(256) for _ in range(3))

        return _decode_doom_patch(titlepic_data, palette)
    finally:
        if mm is not None:
            mm.close()
        if fh is not None:
            fh.close()
