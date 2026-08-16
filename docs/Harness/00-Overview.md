# Harness — how dictate is built and changed

The HOW plane. Nothing here is externally observable; if a statement could be
checked by a black-box tester, it belongs in the [FSD](../Functionality/FSD.md)
instead.

## 1. The constraint that shapes everything

**There is no .NET SDK in the devcontainer.** This was decided deliberately
(`decisions.md` D-15) and it has consequences you will feel within ten minutes:

- `dotnet build`, `dotnet test` and `dotnet run` do not exist locally.
- A compile error is discovered by GitHub Actions, roughly 2–3 minutes after you
  push.
- VS Code gives you syntax highlighting but **no IntelliSense**, because the C#
  language server needs the SDK.

Two habits follow, and they are not optional here:

1. **Prefer APIs you can verify from documentation over ones you half-remember.**
   A guessed method name costs a full CI round trip. Where the shape of an SDK
   call is uncertain, reach for a BCL equivalent that cannot drift — the cleanup
   timeout uses `Task.WaitAsync` rather than the Anthropic client's own timeout
   property for exactly this reason.
2. **Push compile-checkable changes in batches.** One push that fixes six
   plausible errors beats six pushes fixing one each.

To reverse the decision, add the `dotnet` feature to `.devcontainer/devcontainer.json`
and rebuild. Nothing else in the repository assumes its absence.

## 2. Project layout

```
src/Dictate.Core/      Platform-free. Compiles and is tested on Linux.
src/Dictate.Windows/   Every Win32 call. net9.0-windows, WinExe.
tests/Dictate.Core.Tests/   xunit, host tier.
```

**The rule that keeps CI useful: `Dictate.Core` may not reference a Windows-only
API.** It is enforced by the fact that CI builds and tests it on `ubuntu-latest`
(FR-20.3) — break the rule and the build goes red, which is the point. If you
find yourself wanting `System.Windows.Forms` in `Core`, the logic wants to move
to `Windows`, or the Win32 part wants an interface in `Core`.

`Dictate.Windows` sets `EnableWindowsTargeting` so a Linux runner can restore and
compile it. It compiles there; it cannot run there.

## 3. Seams, and why they exist

| Interface | In `Core` | Exists so that |
|---|---|---|
| `ITranscriber` | yes | A streaming Scribe backend can replace the batch one without touching the pipeline (D-16) |
| `ICleaner` | yes | Cleanup can be swapped, benchmarked, or disabled entirely (`PassthroughCleaner`) |

Both are the reason `DictationPipeline` is testable without a network. Add a new
external dependency and it gets an interface in `Core` first.

## 4. Testing standard

### Tiers

| Tier | Where | When | What it can reach |
|---|---|---|---|
| `host` | `ubuntu-latest` | Every push | All of `Dictate.Core` |
| `ci` | `windows-latest` | On tag | Compilation and publish only |
| `desktop` | The user's Windows 11 machine | By hand | Hotkey, microphone, `SendInput`, focus, tray |

**The `desktop` tier is not automatable in this project's infrastructure**, and
the documentation says so rather than implying a CI job that does not exist. It
needs a real keyboard hook, a real microphone and a real foreground window.

### Writing a host test

- One behaviour per test; the name states the behaviour, not the method.
- Use the stubs in `PipelineTests` rather than mocking frameworks — there are two
  interfaces and they have one method each.
- **Assert the prohibited outcome, not just the expected one.** `FR-6.4` is
  discharged by asserting the delivered text contains *no* newline; asserting it
  equals the happy-path string would pass while a newline slipped through in a
  case nobody thought of.

### Adding a test

Declare it in [`../../testing/test-plan.yaml`](../../testing/test-plan.yaml) with
the requirement IDs it discharges, then implement it. A test that exists but is
not declared is invisible to the traceability matrix, which means the requirement
still reads as unverified — correctly, because nobody can find its evidence.

### Order

Verification contract → plan declaration → executable test → code. **Per
requirement, not as project phases.** Running the steps as phases produces a plan
that reads finished while nothing is verified.

## 5. Conventions

- Nullable reference types are on. Do not sprinkle `!` to silence them; if a value
  can be null, the signature should say so.
- `TreatWarningsAsErrors` is deliberately **off** — with CI-only builds, a warning
  promoted to an error costs a round trip to discover.
- Comments explain constraints the code cannot show — why `dwExtraInfo` is
  stamped, why suppression defaults off, why cleanup has no `cache_control`. They
  do not narrate the next line.
- Match the surrounding style rather than importing a new one.

## 6. Release

Tag `v*` on `main`. The workflow builds on `windows-latest`, publishes a
self-contained single-file exe, and attaches it to a GitHub release. Version comes
from the tag; nothing is hand-edited.
