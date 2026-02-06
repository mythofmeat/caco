"""Pydantic models for idgames API responses."""

from pydantic import BaseModel, Field, field_validator

from caco.utils import coerce_str as _coerce_str


class Review(BaseModel):
    """A review for a file."""

    text: str = ""
    vote: int | None = None
    username: str = ""

    @field_validator("text", "username", mode="before")
    @classmethod
    def coerce_str(cls, v):
        return _coerce_str(v)


class FileEntry(BaseModel):
    """File metadata from the idgames archive."""

    id: int
    title: str = ""
    dir: str = ""
    filename: str = ""
    size: int = 0
    age: int = 0
    date: str = ""
    author: str = ""
    email: str = ""
    description: str = ""
    credits: str = ""
    base: str = ""
    buildtime: str = ""
    editors: str = ""
    bugs: str = ""
    textfile: str = ""
    rating: float = 0.0
    votes: int = 0
    url: str = ""
    idgamesurl: str = ""
    reviews: list[Review] = Field(default_factory=list)

    @field_validator(
        "title", "dir", "filename", "date", "author", "email", "description",
        "credits", "base", "buildtime", "editors", "bugs", "textfile", "url", "idgamesurl",
        mode="before"
    )
    @classmethod
    def coerce_str(cls, v):
        return _coerce_str(v)

    @field_validator("rating", mode="before")
    @classmethod
    def coerce_rating(cls, v):
        return 0.0 if v is None else v

    @field_validator("votes", "size", "age", mode="before")
    @classmethod
    def coerce_int(cls, v):
        return 0 if v is None else v


class Directory(BaseModel):
    """Directory info from the idgames archive."""

    id: int
    name: str


class Vote(BaseModel):
    """A vote entry."""

    id: int = 0
    file: int = 0
    title: str = ""
    author: str = ""
    description: str = ""
    rating: float = 0.0
    reviewtext: str = ""

    @field_validator("title", "author", "description", "reviewtext", mode="before")
    @classmethod
    def coerce_str(cls, v):
        return _coerce_str(v)

    @field_validator("rating", mode="before")
    @classmethod
    def coerce_rating(cls, v):
        return 0.0 if v is None else v


class ApiInfo(BaseModel):
    """API information from the about endpoint."""

    version: str = ""
    credits: str = ""
    copyright: str = ""
