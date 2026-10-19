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
| [`../testing/test-plan.yaml`](../testing/test-plan.yaml) | Every test: its tier, the requirements it discharges, what it last produced |

## Routing rule

Ask in order, first yes wins:

1. Externally observable? → **FSD**
2. Constrains how code is written or verified? → **Harness**
3. Tells a human how to run or recover it? → **UserDocumentation**
4. Why does it work this way? → the FSD §1.4 Design basis (product) or Harness §0 (implementation), beside the thing it explains
5. What it used to do, when it changed, or who decided it? → the git history. **The planes carry only what is true now**, in the present tense — no dates, no version narratives, no record of superseded behaviour or of who decided what. A reason is stated as a present constraint. Code comments follow the same rule. Test-plan evidence is the exception: it records results, so it keeps its dates and runs

Two questions settle the hard cases: *could a black-box tester verify it?*
(yes → WHAT) and *would it survive a rewrite in another language?* (no → HOW).
