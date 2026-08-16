# dictate — working notes for Claude

Push-to-talk dictation for Windows. C#/.NET 9. See
[`docs/00-Overview.md`](docs/00-Overview.md) for the documentation map; this file
is only about collaborating on the repository.

## The one thing that will catch you out

**There is no .NET SDK in this devcontainer.** Deliberate — see
`docs/Harness/00-Overview.md` §0. So:

- `dotnet build` / `dotnet test` / `dotnet run` do not exist here.
- Compile errors surface in GitHub Actions ~2–3 minutes after a push.
- VS Code has no C# IntelliSense in this container.

Work accordingly: prefer API shapes you can verify from documentation over ones
you half-remember, and batch compile-checkable changes into one push rather than
six. Where an SDK's own surface is uncertain, reach for a BCL equivalent that
cannot drift — `HaikuCleaner` bounds its call with `Task.WaitAsync` rather than
the Anthropic client's timeout property, for exactly this reason.

## Layout

| Path | What |
|---|---|
| `src/Dictate.Core/` | Platform-free. Compiles **and is tested** on Linux. |
| `src/Dictate.Windows/` | Every Win32 call. `net9.0-windows`, WinExe. |
| `tests/Dictate.Core.Tests/` | xunit, host tier, runs in CI on every push. |
| `docs/` | Three planes — WHAT / HOW / OPERATE. Start at `00-Overview.md`. Rationale sits beside what it explains: FSD §1.4, Harness §0. |
| `testing/test-plan.yaml` | Every test, its tier, and the requirements it discharges. |

**`Dictate.Core` must not reference a Windows-only API.** CI enforces it by
compiling and testing Core on `ubuntu-latest`. If you want `System.Windows.Forms`
in Core, either the logic belongs in `Windows`, or the Win32 part needs an
interface in `Core`.

## Before changing behaviour

Check `docs/Functionality/FSD.md` — the behaviour is probably already specified,
and three requirements exist because getting them wrong is expensive:

- **FR-6.4** — no newline is ever typed into a console. Enter in a shell runs the
  line. This is structural, not a prompt instruction, and `TextSanitizerTests`
  asserts the *prohibited* outcome, not just the expected one.
- **FR-6.1/6.2** — the target window is pinned at key-press. If focus moved by
  delivery time, the text goes to the clipboard and *nothing* is typed.
- **FR-7.1** — cleanup failure degrades to the raw transcript. A scruffy sentence
  beats a lost one.

Changing any of these means updating the FSD in the same commit, not afterwards.

## Adding a test

Declare it in `testing/test-plan.yaml` with the requirement IDs it discharges,
then implement it. An undeclared test is invisible to traceability, so its
requirement still reads as unverified — correctly, because nobody can find the
evidence.

Note the honest position recorded at the bottom of that file: **nothing has been
executed yet**. Implemented ≠ verified.

## Conventions

- Nullable reference types on. Don't silence them with `!`.
- `TreatWarningsAsErrors` is off on purpose — with CI-only builds a promoted
  warning costs a round trip to discover.
- Comments state constraints the code cannot show (why `dwExtraInfo` is stamped,
  why hotkey suppression defaults off). They don't narrate the next line.

## Don't

- Don't add a `.ico` — tray icons are drawn at runtime in `TrayIcons.cs`.
- Don't make the app elevated. An elevated hook cannot see input bound for
  unelevated windows, so dictation would stop working in ordinary apps.
- Don't write audio or transcripts to disk. FR-17.1 is a user decision, not an
  oversight, and the in-memory ring buffer is the agreed compromise.
