# Documentation map

Three planes, three questions, three readers. Every sentence belongs to exactly
one of them — mixing them is why specifications rot.

| Plane | Question | Where | Read it when |
|---|---|---|---|
| **WHAT** | What must be true? | [`Functionality/FSD.md`](Functionality/FSD.md) | You need to know the required behaviour, or whether something is a bug |
| **HOW** | How is it built and changed? | [`Harness/00-Overview.md`](Harness/00-Overview.md) | You are about to write code, a test, or a workflow |
| **OPERATE** | How do I run it? | [`UserDocumentation/Manual.md`](UserDocumentation/Manual.md) | You want to install it, use it, or recover from something |

Not planes, but load-bearing:

| File | Contents |
|---|---|
| [`decisions.md`](decisions.md) | What was decided in the design interview, what was rejected, and why |
| [`open-issues.md`](open-issues.md) | Questions still unanswered, each with who can answer it |
| [`../testing/test-plan.yaml`](../testing/test-plan.yaml) | Every test: its tier, the requirements it discharges, what it last produced |
| `../CLAUDE.md` | How to collaborate with the assistant on this repository |

## Routing rule

Ask in order, first yes wins:

1. Externally observable? → **FSD**
2. Constrains how code is written or verified? → **Harness**
3. Tells a human how to run or recover it? → **UserDocumentation**
4. About working with the assistant? → `CLAUDE.md`
5. Why a past decision was made? → `decisions.md` or the commit message

Two questions settle the hard cases: *could a black-box tester verify it?*
(yes → WHAT) and *would it survive a rewrite in another language?* (no → HOW).
