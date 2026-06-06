## Summary

<!-- What does this PR change, and why? Keep it concise. -->

## Related issue

<!-- Link the issue this PR addresses, e.g. "Closes #123". Write "N/A" if there is none. -->

## Type of change

<!-- Mark all that apply with an [x]. -->

- [ ] Bug fix (non-breaking change that fixes an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (fix or feature that changes existing behavior)
- [ ] Refactor / internal cleanup (no behavior change)
- [ ] Documentation only

## How tested

<!-- Describe how you verified the change. -->

- [ ] `cargo test --release` passes
- [ ] For parser changes: verified against real `~/.claude` data (e.g. the `/verify-parser` skill, or `./target/release/arrow --list`)
- [ ] For UI changes: checked in the egui app (`cargo run --manifest-path gui/Cargo.toml`)

<!-- Add any extra detail about your testing here. -->

## Checklist

- [ ] `cargo fmt --all --check` leaves no changes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes (CI treats any clippy warning as an error)
- [ ] New code comments, identifiers, and UI text are in English
- [ ] Change respects arrow's **honesty principle** (does not claim more than the native data knows; only `Edit`/`Write`/`MultiEdit` edits are captured, never `Bash` changes)
- [ ] Documentation updated where needed (README.md / ROADMAP.md / SPEC.md / CLAUDE.md)
- [ ] Commit messages are in the imperative mood
