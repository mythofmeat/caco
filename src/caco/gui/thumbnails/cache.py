"""Thumbnail filesystem cache at ~/.cache/caco/thumbnails/."""

from pathlib import Path

THUMBNAIL_DIR = Path.home() / ".cache" / "caco" / "thumbnails"


def get_cache_path(wad_id: int) -> Path:
    """Get the expected cache path for a WAD's thumbnail."""
    return THUMBNAIL_DIR / f"{wad_id}.png"


def is_cached(wad_id: int) -> bool:
    """Check if a thumbnail is already cached."""
    return get_cache_path(wad_id).exists()


def save(wad_id: int, image_bytes: bytes) -> Path:
    """Save thumbnail bytes to the cache. Returns the file path."""
    THUMBNAIL_DIR.mkdir(parents=True, exist_ok=True)
    path = get_cache_path(wad_id)
    path.write_bytes(image_bytes)
    return path


def load(wad_id: int) -> bytes | None:
    """Load cached thumbnail bytes. Returns None if not cached."""
    path = get_cache_path(wad_id)
    if path.exists():
        return path.read_bytes()
    return None


def clear(wad_id: int) -> bool:
    """Remove a cached thumbnail. Returns True if deleted."""
    path = get_cache_path(wad_id)
    if path.exists():
        path.unlink()
        return True
    return False


def clear_all() -> int:
    """Remove all cached thumbnails. Returns count deleted."""
    if not THUMBNAIL_DIR.exists():
        return 0
    count = 0
    for f in THUMBNAIL_DIR.glob("*.png"):
        f.unlink()
        count += 1
    return count
