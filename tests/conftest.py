import pytest

from agent_qa.models import Contract
from agent_qa.store import Store


@pytest.fixture
def contract():
    return Contract(
        title="Save flow",
        target_kind="web",
        build_id="fixture-v1",
        scope="Save and reload the profile",
        criteria=[{"id": "save", "description": "Saved value persists"}],
        max_steps=20,
    )


@pytest.fixture
def setup(tmp_path, contract):
    workspace = tmp_path / "workspace"
    workspace.mkdir()
    store = Store(tmp_path / "data")
    job = store.create(contract, str(workspace))
    run = store.begin_run(job["id"], "discovery", "Fresh fixture and profile")
    return store, job, run, workspace
