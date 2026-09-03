from gitcell.storage import InteractionStore


def test_record_and_list(tmp_path):
    store = InteractionStore(tmp_path)
    store.init()

    store.record("user", "please add a feature")
    store.record("agent", "sure, here is the plan", metadata={"tokens": 42})

    interactions = store.list()
    assert len(interactions) == 2
    assert interactions[0].role == "user"
    assert interactions[0].content == "please add a feature"
    assert interactions[1].metadata == {"tokens": 42}


def test_list_filters_by_role(tmp_path):
    store = InteractionStore(tmp_path)
    store.record("user", "hi")
    store.record("agent", "hello")
    store.record("user", "how are you")

    user_only = store.list(role="user")
    assert len(user_only) == 2
    assert all(i.role == "user" for i in user_only)


def test_list_respects_limit(tmp_path):
    store = InteractionStore(tmp_path)
    for i in range(5):
        store.record("user", f"message {i}")

    limited = store.list(limit=2)
    assert len(limited) == 2
    # Most recent two, in chronological order.
    assert limited[-1].content == "message 4"
