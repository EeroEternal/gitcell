import textwrap

from gitcell.workflow import WorkflowError, WorkflowRunner


def _write_workflow(tmp_path, content):
    workflows_dir = tmp_path / ".gitcell" / "workflows"
    workflows_dir.mkdir(parents=True, exist_ok=True)
    (workflows_dir / "ci.yml").write_text(textwrap.dedent(content))


def test_discover_and_run_success(tmp_path):
    _write_workflow(
        tmp_path,
        """
        name: CI
        on: [push]
        env:
          GREETING: hi
        jobs:
          build:
            steps:
              - name: greet
                run: echo "$GREETING"
        """,
    )

    runner = WorkflowRunner(tmp_path)
    workflows = runner.discover()
    assert len(workflows) == 1
    assert workflows[0].name == "CI"
    assert workflows[0].events == ["push"]

    result = runner.run(workflows[0], event="push")
    assert result.ok
    assert result.jobs[0].steps[0].stdout.strip() == "hi"


def test_run_stops_on_failure(tmp_path):
    _write_workflow(
        tmp_path,
        """
        name: CI
        on: [push]
        jobs:
          build:
            steps:
              - name: fail
                run: exit 1
              - name: never-runs
                run: echo "should not print"
        """,
    )

    runner = WorkflowRunner(tmp_path)
    workflow = runner.find("CI")
    result = runner.run(workflow, event="push")
    assert not result.ok
    assert len(result.jobs[0].steps) == 1


def test_event_mismatch_raises(tmp_path):
    _write_workflow(
        tmp_path,
        """
        name: CI
        on: [push]
        jobs:
          build:
            steps:
              - name: noop
                run: echo hi
        """,
    )
    runner = WorkflowRunner(tmp_path)
    workflow = runner.find("CI")
    try:
        runner.run(workflow, event="pull_request")
        assert False, "expected WorkflowError"
    except WorkflowError:
        pass


def test_find_missing_workflow_raises(tmp_path):
    runner = WorkflowRunner(tmp_path)
    try:
        runner.find("does-not-exist")
        assert False, "expected WorkflowError"
    except WorkflowError:
        pass
