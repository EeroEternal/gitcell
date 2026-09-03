import subprocess

from gitcell.git_ops import GitRepo


def _init_repo(path):
    repo = GitRepo(path)
    repo.init()
    subprocess.run(["git", "config", "user.email", "test@example.com"], cwd=path, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=path, check=True)
    return repo


def test_init_and_status(tmp_path):
    repo = _init_repo(tmp_path)
    assert repo.is_git_repo()
    status = repo.status().lower()
    assert status == "" or "branch" in status or "no commits yet" in status


def test_add_and_commit(tmp_path):
    repo = _init_repo(tmp_path)
    (tmp_path / "file.txt").write_text("hello\n")
    repo.add("file.txt")
    result = repo.commit("initial commit")
    assert result.ok
    log = repo.log()
    assert "initial commit" in log
