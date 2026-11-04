"""The service contract is intentionally small and explicit."""

from typing import Literal
from urllib.parse import urlsplit

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator


class StrictModel(BaseModel):
    model_config = ConfigDict(extra="forbid", str_strip_whitespace=True)


class Criterion(StrictModel):
    id: str = Field(pattern=r"^[a-zA-Z0-9_-]{1,64}$")
    description: str = Field(min_length=1, max_length=4000)


class Contract(StrictModel):
    title: str = Field(min_length=1, max_length=200)
    target_kind: Literal["web", "mobile", "desktop", "game", "software"]
    build_id: str = Field(min_length=1, max_length=500)
    scope: str = Field(min_length=1, max_length=10000)
    criteria: list[Criterion] = Field(min_length=1, max_length=200)
    allowed_origins: list[str] = Field(default_factory=list, max_length=30)
    max_steps: int = Field(default=100, ge=1, le=10000)
    max_seconds: int = Field(default=3600, ge=1, le=604800)
    # This covers only explicitly reported costs; host billing is not available through MCP.
    max_reported_cost_usd: float = Field(default=10, ge=0, le=100000, allow_inf_nan=False)

    @field_validator("allowed_origins")
    @classmethod
    def origins(cls, values: list[str]) -> list[str]:
        result = []
        for value in values:
            p = urlsplit(value)
            if (
                p.scheme not in {"http", "https"}
                or not p.hostname
                or p.username
                or p.password
                or p.path not in {"", "/"}
                or p.query
                or p.fragment
            ):
                raise ValueError(
                    "Use exact http(s) origins, without credentials, paths, queries or fragments"
                )
            # Accessing port validates malformed ports as well.
            port = p.port
            hostname = p.hostname.lower()
            hostname = f"[{hostname}]" if ":" in hostname else hostname
            suffix = f":{port}" if port and port != {"http": 80, "https": 443}[p.scheme] else ""
            result.append(f"{p.scheme}://{hostname}{suffix}")
        return list(dict.fromkeys(result))

    @model_validator(mode="after")
    def unique_criteria(self):
        ids = [c.id for c in self.criteria]
        if len(ids) != len(set(ids)):
            raise ValueError("Criterion IDs must be unique")
        return self


class BrowserAction(StrictModel):
    kind: Literal["navigate", "click", "fill", "press", "wait_for", "observe"]
    url: str | None = Field(default=None, max_length=4000)
    selector: str | None = Field(default=None, max_length=1000)
    value: str | None = Field(default=None, max_length=10000)
    timeout_ms: int = Field(default=5000, ge=100, le=30000)

    @model_validator(mode="after")
    def arguments(self):
        if self.kind == "navigate" and not self.url:
            raise ValueError("navigate needs url")
        if self.kind in {"click", "fill", "press", "wait_for"} and not self.selector:
            raise ValueError("action needs selector")
        if self.kind in {"fill", "press"} and self.value is None:
            raise ValueError("fill/press needs value")
        return self


class Checkpoint(StrictModel):
    summary: str = Field(min_length=1, max_length=12000)
    explored: list[str] = Field(default_factory=list, max_length=200)
    hypotheses: list[str] = Field(default_factory=list, max_length=200)
    next_actions: list[str] = Field(default_factory=list, max_length=50)


class FindingInput(StrictModel):
    title: str = Field(min_length=1, max_length=300)
    expected: str = Field(min_length=1, max_length=8000)
    actual: str = Field(min_length=1, max_length=8000)
    steps: list[str] = Field(min_length=1, max_length=200)
    severity: Literal["critical", "high", "medium", "low"]
    criterion_id: str
    artifact_ids: list[str] = Field(min_length=1, max_length=50)


class Outcome(StrictModel):
    criterion_id: str
    result: Literal["pass", "fail", "inconclusive", "not_tested"]
    reason: str = Field(min_length=1, max_length=4000)
    artifact_ids: list[str] = Field(default_factory=list, max_length=50)
