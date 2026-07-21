from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path

import pytest

import freeze_tb21_study_seed as freezer
from tb21_evidence_contract import (
    build_task_partition,
    canonical_body_bytes,
    canonical_file_bytes,
    parse_canonical_object,
    validate_task_partition,
)
from tb21_study_seed import TASK_IDENTITIES, TASK_SET_HASH_DOMAIN, task_set_sha256


def _write_json(path: Path, value: object) -> bytes:
    raw = (json.dumps(value, sort_keys=True) + "\n").encode()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(raw)
    return raw


def _freezer_comparator_fixture(
    root: Path, *, trial_count: int = 2
) -> tuple[Path, dict[str, dict[str, object]]]:
    comparator = root / "comparator"
    entries: list[dict[str, object]] = []
    trial_ids: list[str] = []
    for index in range(trial_count):
        trial_id = f"trial-{index}"
        trial_ids.append(trial_id)
        task_name = f"task-{index}"
        result = {field: None for field in freezer._RESULT_FIELDS}
        result.update(
            {
                "id": f"result-{index}",
                "task_checksum": hashlib.sha256(
                    f"checksum-{index}".encode()
                ).hexdigest(),
                "task_id": {
                    "org": "terminal-bench",
                    "name": task_name,
                    "ref": "sha256:"
                    + hashlib.sha256(f"ref-{index}".encode()).hexdigest(),
                },
                "task_name": f"terminal-bench/{task_name}",
                "trial_name": f"{task_name}__trial",
            }
        )
        result_raw = _write_json(
            comparator / "trials" / trial_id / "result.json", result
        )
        entry = {field: None for field in freezer._MANIFEST_ENTRY_FIELDS}
        entry.update(
            {
                "submitted_trial_id": trial_id,
                "result_id": result["id"],
                "trial_name": result["trial_name"],
                "task_name": result["task_name"],
                "result_sha256": hashlib.sha256(result_raw).hexdigest(),
                "result_bytes": len(result_raw),
            }
        )
        entries.append(entry)
    manifest: dict[str, object] = {
        "leaderboard_job_id": freezer.LEADERBOARD_JOB_ID,
        "submission_url": "https://invalid.example/pinned",
        "entries": entries,
        "failures": [],
    }
    submission: dict[str, object] = {"trials": trial_ids}
    return comparator, {
        "manifest.json": manifest,
        "submission.json": submission,
    }


def _refresh_split_digest(partition: dict[str, object], split: str) -> None:
    digests = partition["split_sha256"]
    assert isinstance(digests, dict)
    digests[split] = hashlib.sha256(canonical_body_bytes(partition[split])).hexdigest()


def test_real_seed_and_partition_are_frozen() -> None:
    # This is an internal hash-domain label, not an artifact schema version.
    assert TASK_SET_HASH_DOMAIN == "stella-tb21-task-set-v1"
    assert len(TASK_IDENTITIES) == 89
    assert task_set_sha256(TASK_IDENTITIES) == (
        "7e495afe0a86eaf572be1c2da2b9929c24e502adc888e550385d915cc0125ece"
    )
    partition = build_task_partition(TASK_IDENTITIES)
    assert [
        len(partition[name]) for name in ("development", "screen", "untouched")
    ] == [10, 20, 59]
    assert partition["split_sha256"] == {
        "development": (
            "265ef7896a287493fd846b5835d8eecb83e0e1dd74036aebd4c8e603cf5d3105"
        ),
        "screen": ("48828ea2c4fab2b7791a1b4e76e7d764c18cc94efb631bc944325aa91ace9866"),
        "untouched": (
            "324cfb122eb8220b4f7a177a932f1af45e5e4948fc22c9294156477d157bc26e"
        ),
    }
    screen = partition["screen"]
    assert isinstance(screen, list)
    assert [item["task_name"] for item in screen] == [
        "extract-moves-from-video",
        "pytorch-model-recovery",
        "dna-assembly",
        "path-tracing-reverse",
        "extract-elf",
        "build-cython-ext",
        "polyglot-c-py",
        "sparql-university",
        "polyglot-rust-c",
        "sqlite-db-truncate",
        "password-recovery",
        "build-pmars",
        "qemu-startup",
        "largest-eigenval",
        "regex-chess",
        "model-extraction-relu-logits",
        "mailman",
        "git-multibranch",
        "nginx-request-logging",
        "protein-assembly",
    ]
    assert validate_task_partition(partition) == partition


def test_generated_seed_names_the_internal_hash_domain() -> None:
    source = freezer._source_bytes(TASK_IDENTITIES)

    assert b'TASK_SET_HASH_DOMAIN = "stella-tb21-task-set-v1"' in source
    assert b"Internal hash-domain label; not an artifact schema version." in source


def test_local_verification_rejects_comparator_reference_drift(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    checksums = {name: checksum for name, _task_ref, checksum in TASK_IDENTITIES}
    for task_name in checksums:
        (tmp_path / task_name).mkdir()

    class FakeTask:
        def __init__(self, task_dir: Path) -> None:
            self.checksum = checksums[task_dir.name]

    monkeypatch.setattr(freezer, "Task", FakeTask)
    drifted = list(TASK_IDENTITIES)
    name, _task_ref, checksum = drifted[0]
    drifted[0] = (name, "sha256:" + "0" * 64, checksum)

    with pytest.raises(RuntimeError, match="pinned task identity binding"):
        freezer._verify_local_dataset(tmp_path, tuple(drifted))


def test_freezer_trial_count_is_frozen() -> None:
    assert freezer.EXPECTED_TRIAL_COUNT == 445


def test_freezer_rejects_control_digest_drift(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    original: dict[str, bytes] = {}
    digests: dict[str, str] = {}
    for filename in freezer.CONTROL_SHA256:
        raw = _write_json(tmp_path / filename, {"file": filename})
        original[filename] = raw
        digests[filename] = hashlib.sha256(raw).hexdigest()
    monkeypatch.setattr(freezer, "CONTROL_SHA256", digests)

    assert set(freezer._load_control_files(tmp_path)) == set(digests)
    (tmp_path / "manifest.json").write_bytes(original["manifest.json"] + b" ")
    with pytest.raises(RuntimeError, match="control digest differs"):
        freezer._load_control_files(tmp_path)


@pytest.mark.parametrize("target", ["manifest", "entry", "result"])
def test_freezer_rejects_fields_outside_strict_allowlists(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, target: str
) -> None:
    comparator, controls = _freezer_comparator_fixture(tmp_path)
    monkeypatch.setattr(freezer, "EXPECTED_TRIAL_COUNT", 2)
    manifest = controls["manifest.json"]
    entries = manifest["entries"]
    assert isinstance(entries, list)
    first_entry = entries[0]
    assert isinstance(first_entry, dict)
    if target == "manifest":
        manifest["unexpected"] = True
    elif target == "entry":
        first_entry["unexpected"] = True
    else:
        trial_id = first_entry["submitted_trial_id"]
        assert isinstance(trial_id, str)
        result_path = comparator / "trials" / trial_id / "result.json"
        result = json.loads(result_path.read_bytes())
        result["unexpected"] = True
        result_raw = _write_json(result_path, result)
        first_entry["result_sha256"] = hashlib.sha256(result_raw).hexdigest()
        first_entry["result_bytes"] = len(result_raw)

    with pytest.raises(RuntimeError, match="fields differ"):
        freezer._manifest_trials(comparator, controls)


@pytest.mark.parametrize(
    ("mutation", "message"),
    [
        ("count", "exactly 2"),
        ("identity", "inconsistent result ID"),
        ("submission", "submitted trial allowlist"),
    ],
)
def test_freezer_rejects_count_and_identity_linkage_drift(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    mutation: str,
    message: str,
) -> None:
    comparator, controls = _freezer_comparator_fixture(tmp_path)
    monkeypatch.setattr(freezer, "EXPECTED_TRIAL_COUNT", 2)
    manifest = controls["manifest.json"]
    entries = manifest["entries"]
    submission = controls["submission.json"]
    trial_ids = submission["trials"]
    assert isinstance(entries, list)
    assert isinstance(trial_ids, list)
    if mutation == "count":
        entries.pop()
    elif mutation == "identity":
        entry = entries[0]
        assert isinstance(entry, dict)
        entry["result_id"] = "wrong-result"
    else:
        trial_ids[-1] = "unexpected-trial"

    with pytest.raises(RuntimeError, match=message):
        freezer._manifest_trials(comparator, controls)


@pytest.mark.parametrize("mutation", ["extra", "missing"])
def test_freezer_rejects_extra_or_missing_trial_directories(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    mutation: str,
) -> None:
    comparator, controls = _freezer_comparator_fixture(tmp_path)
    monkeypatch.setattr(freezer, "EXPECTED_TRIAL_COUNT", 2)
    if mutation == "extra":
        (comparator / "trials" / "unexpected").mkdir()
        message = "extra or missing trials"
    else:
        missing = comparator / "trials" / "trial-1" / "result.json"
        missing.unlink()
        message = "cannot read comparator result"

    with pytest.raises(RuntimeError, match=message):
        freezer._manifest_trials(comparator, controls)


def test_freezer_requires_harbor_061(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(freezer.harbor, "__version__", "0.6.2")

    with pytest.raises(RuntimeError, match="Harbor 0.6.1 is required"):
        freezer._verify_local_dataset(tmp_path, ())


def test_freezer_output_is_create_or_identical(tmp_path: Path) -> None:
    output = tmp_path / "seed.py"

    assert freezer._create_or_verify_identical(output, b"frozen\n") == "created"
    assert freezer._create_or_verify_identical(output, b"frozen\n") == "identical"
    output.write_bytes(b"drifted\n")
    with pytest.raises(RuntimeError, match="refusing to overwrite"):
        freezer._create_or_verify_identical(output, b"frozen\n")


def test_canonical_json_has_distinct_file_and_body_encodings() -> None:
    value = {"z": "café", "a": [1, True, None]}
    body = b'{"a":[1,true,null],"z":"caf\xc3\xa9"}'

    assert canonical_body_bytes(value) == body
    assert canonical_file_bytes(value) == body + b"\n"
    assert parse_canonical_object(body + b"\n", label="fixture") == value


@pytest.mark.parametrize(
    "raw",
    [
        b'{"a":1,"a":2}\n',
        b"[]\n",
        b'{"z":2,"a":1}\n',
        b'{"a":1}',
        b'{"a":NaN}\n',
        b'{"a":Infinity}\n',
        b'{"a":-Infinity}\n',
    ],
)
def test_strict_parser_rejects_ambiguous_or_noncanonical_json(raw: bytes) -> None:
    with pytest.raises(ValueError, match="fixture"):
        parse_canonical_object(raw, label="fixture")


def test_strict_parser_normalizes_lone_surrogate_reencoding_failure() -> None:
    raw = b'{"value":"\\ud800"}\n'

    with pytest.raises(ValueError, match="fixture contains invalid Unicode"):
        parse_canonical_object(raw, label="fixture")


@pytest.mark.parametrize("field", ["schema_version", "study_id", "development"])
def test_partition_rejects_missing_fields(field: str) -> None:
    partition = build_task_partition(TASK_IDENTITIES)
    del partition[field]

    with pytest.raises(ValueError):
        validate_task_partition(partition)


def test_partition_rejects_extra_top_level_and_record_fields() -> None:
    partition = build_task_partition(TASK_IDENTITIES)
    partition["unexpected"] = True
    with pytest.raises(ValueError):
        validate_task_partition(partition)

    partition = build_task_partition(TASK_IDENTITIES)
    development = partition["development"]
    assert isinstance(development, list)
    development[0]["unexpected"] = True
    _refresh_split_digest(partition, "development")
    with pytest.raises(ValueError):
        validate_task_partition(partition)


def test_partition_rejects_duplicate_task_names_and_references() -> None:
    partition = build_task_partition(TASK_IDENTITIES)
    development = partition["development"]
    assert isinstance(development, list)
    development[1]["task_name"] = development[0]["task_name"]
    _refresh_split_digest(partition, "development")
    with pytest.raises(ValueError, match="duplicate task name"):
        validate_task_partition(partition)

    partition = build_task_partition(TASK_IDENTITIES)
    development = partition["development"]
    assert isinstance(development, list)
    development[1]["canonical_task_reference"] = development[0][
        "canonical_task_reference"
    ]
    _refresh_split_digest(partition, "development")
    with pytest.raises(ValueError, match="duplicate task reference"):
        validate_task_partition(partition)


def test_partition_rejects_incorrect_split_digest() -> None:
    partition = build_task_partition(TASK_IDENTITIES)
    digests = partition["split_sha256"]
    assert isinstance(digests, dict)
    digests["screen"] = "0" * 64

    with pytest.raises(ValueError, match="split digest"):
        validate_task_partition(partition)


def test_partition_rejects_a_digest_consistent_nonfrozen_split() -> None:
    partition = copy.deepcopy(build_task_partition(TASK_IDENTITIES))
    untouched = partition["untouched"]
    assert isinstance(untouched, list)
    untouched[0]["task_checksum"] = "0" * 64
    _refresh_split_digest(partition, "untouched")

    with pytest.raises(ValueError, match="frozen seed"):
        validate_task_partition(partition)
