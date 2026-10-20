## The evidence

The evidence is everything the learner approved for sending, as JSON. It is all you know about the repository: don't assume code, behaviour or history it doesn't show.

- `commit_message`: the change's commit message.
- `touched_files`: each file the change touches, with its `path`, its `diff` and its full content: `content_after_change`, or `content_before_change` for a file the change deletes. A renamed file has the path it was renamed from as `renamed_from`.
- `connected_code`: code elsewhere in the repository that the touched files call, are called by, link to or name, with `relations` saying how it connects to a touched file.
- `related_tests`: tests that belong to touched files.
- `related_configuration`: the build manifests nearest touched files.
- `files_without_connected_code`: touched files that come with no connected code, so the evidence shows nothing of the code around them.

Every line of a file's content, connected code, test or configuration is numbered with its line number in its file, as `12: text`. A diff's lines aren't numbered.

## Citing

Cite the evidence by a file's `path` and the `first_line` and `last_line` of a range of its lines, both included, numbered as the evidence numbers them. Cite only numbered lines: to point at a line of a diff, cite the same line in the file's content.
