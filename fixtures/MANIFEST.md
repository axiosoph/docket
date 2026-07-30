# Fixture manifest

Each subdirectory of `fixtures/` is a **self-contained corpus root**: its
own `docket.ncl` plus a `docs/` tree, meant to be pointed at as one
invocation (`docket check fixtures/<name>/`). Corpus roots never share
state — a document stem repeated across two *different* fixture
directories is not a collision; only stems repeated *within* one root are.

Every failing fixture is minimal in the strict sense stated in the
dispatch: **remove any line from it and it stops isolating the named
check** — either because the fixture now passes outright, or because a
*different* check would start firing instead (both count as "no longer
isolates check X alone"). Where a fixture needs more than the bare
minimum to prevent a second, unrelated check from also firing, that is
called out below.

| fixture | fails | why |
|:---|:---|:---|
| `c1-unknown-field/` | `C1` | `docs/specs/example.md`'s claim block has `extra: nope`, a field the contract does not declare. MVP.md §1.2: "Unknown fields are an error, not ignored." The `[example-claim]` heading is present (not omitted) so the block is *not* also orphaned — omitting it would swap the failure to `orphan-claim` rather than removing it, which is why it stays. |
| `c2-duplicate-claim-id/` | `C2` | `docs/specs/a.md` and `docs/specs/b.md` both head a claim `[dup-id]`. Each claim block is independently well-formed, in a genre that permits its kind, with no `cites` — so nothing but the id collision fails. |
| `c3-genre-forbids-kind/` | `C3` | `docket.ncl` declares `docs/adr/**` with `kinds = []` (MVP.md §2's "no claims permitted" form). `docs/adr/0001-decision.md` declares a `requirement` claim there anyway. |
| `c4-dangling-cite/` | `C4` | `docs/specs/a.md`'s claim cites `nonexistent-claim`, which resolves to nothing in the corpus. A prose link `[related work](nonexistent-claim)` with the identical target is present so `C5` (link/cites agreement) still passes — see the C5-vs-C4 judgment call below. |
| `c5-prose-cites-mismatch/` | `C5` | `docs/specs/a.md` declares `[target-claim]` (cited, and it resolves — so `C4` passes) and `[mismatched-claim]`, which cites `[target-claim]` but whose prose body carries no matching link. `L = {}`, `cites = {target-claim}`, sets disagree. `[target-claim]` exists specifically so the citation resolves and `C4` cannot also fire. |
| `orphan-claim/` | `orphan-claim` | `docs/specs/a.md` is a claim block with no preceding heading in the file at all — the minimal case of "no `[id]`-shaped heading precedes it" (MVP.md §1.1). |
| `duplicate-stem/` | `duplicate-stem` | `docs/specs/shared.md` and `docs/models/shared.md` share the stem `shared`. Both files are empty (0 bytes) — no heading or claim block is needed to trigger a stem collision, so none is present; this is the strict-minimality floor. |
| `golden/` | — (all pass) | See below. |

## `golden/`

One document per MVP.md §2's example genre (`docs/specs`, `docs/models`,
`docs/architecture`, `docs/adr`), reproducing the worked example from
MVP.md §4.1 / README.md's claim-block sample almost verbatim:

- `docs/specs/lock-file-schema.md` — claim `[lock-groundness]`
  (`kind: constraint`), citing `composition-model#6` and
  `execution-model#2.4` — the **doc-stem#anchor** reference form, both
  instances also given matching prose links.
- `docs/models/composition-model.md` — hosts the `## 6` heading
  `lock-groundness` cites, plus its own claim `[atom-identity]`
  (`kind: invariant`, `evaluator: proof`, no `cites`) to exercise a second
  kind/evaluator pair.
- `docs/models/execution-model.md` — hosts the `## 2.4` heading
  `lock-groundness` cites. No claim block of its own.
- `docs/architecture/system-overview.md` — claim `[system-boundary]`
  (`kind: requirement`, `evaluator: none`), citing `lock-groundness` — the
  **claim id** reference form, with a matching prose link.
- `docs/adr/0001-use-nickel.md` — a decision record. `kinds = []` for this
  genre, so it carries no claim block, which is the compliant state for
  that genre rather than an exception to it.

All five document stems are distinct, all three claim ids are distinct,
every `cites` entry matches a corresponding prose link, and (assuming the
anchor-resolution reading below) every citation resolves.

## Judgment calls made while writing these fixtures

MVP.md was read as the binding contract per the dispatch; nothing here
required a halt, but three points were ambiguous enough that a call had
to be made. Flagging them here rather than deciding silently:

1. **`cites`-entry syntax, not just C4 resolution, was made part of C1.**
   MVP.md §1.3 defines two reference *shapes* (bare kebab-case claim id;
   `<doc-stem>#<anchor>`) but never states which check enforces the shape
   itself, as opposed to C4's job of resolving the *target*. `contracts/claim.ncl`'s
   `Ref` contract enforces the shape at C1 time (a `cites` entry with
   spaces or a second `#`, e.g., is a *malformed* block, not merely a
   dangling reference) — reasoning that shape-checking is corpus-independent
   and belongs with the other purely-syntactic checks the contract already
   performs. An implementer could instead defer all `cites` shape
   validation to C4, which would also be a defensible reading.

2. **C5's "targets that resolve to corpus documents or claim ids"
   clause was read as a *syntactic* filter, not a *resolution* filter** —
   i.e., a prose link counts toward `L` if it is *shaped* like a claim id
   or a `doc-stem#anchor` pair (as opposed to an external URL, image, or
   in-page anchor), regardless of whether that target actually exists in
   the corpus. The alternative reading — a link only counts toward `L` if
   it successfully resolves — makes `C4` and `C5` **impossible to isolate
   from each other**: any claim with a dangling `cites` entry and a
   matching prose link would fail both checks simultaneously (the link,
   being unresolvable, would be excluded from `L`, so `L` could never
   equal a `cites` set containing a dangling target). Since MVP.md lists
   C4 and C5 as distinct, independently-meaningful checks, and this
   dispatch asked for fixtures that isolate each one, the syntactic
   reading is the one that makes the specification self-consistent. This
   is the single most consequential call in this fixture set — `c4-dangling-cite/`
   only isolates C4 under this reading, and would need to be dropped or
   restructured under the other one. **Flagging for confirmation before
   the extractor is built against it.**

3. **A prose link's target string is assumed to be written literally as
   the same string as a `cites` entry** (e.g. `[text](nonexistent-claim)`,
   `[text](composition-model#6)`) — no `.md` extension, no relative path,
   no leading `#`. MVP.md never shows a worked prose-link example (only
   `cites:` field examples), so there is no anchor to check this against.
   A real implementation might instead expect links written as relative
   markdown paths (`[text](../models/composition-model.md#6)`) and derive
   the doc-stem/claim-id from that — which would change every prose link
   in these fixtures. This call only affects fixture *authoring*, not the
   contracts.

4. **Anchor derivation for `doc-stem#anchor` is unspecified** by MVP.md —
   it says such a reference "resolv[es] to a heading in a corpus document"
   but never states the slug algorithm. `golden/` assumes the anchor is
   the heading's leading token taken literally (`## 6` → anchor `6`,
   `## 2.4` → anchor `2.4`), matching MVP.md's own worked examples
   character-for-character. This only matters once C4 is executable; it
   does not affect anything checked in this dispatch.

## Execution status

**Executed now**, via `nickel export <extracted-block>.yaml --apply-contract
contracts/claim_apply.ncl` for every claim block's YAML across every fixture
(all `docket.ncl` files were also validated against `contracts/docket.ncl`,
which — unlike claim blocks — is plain Nickel and needs no shim). Exit codes
observed, one block per line, `<fixture>_<file>_<block-index-within-file>`:

| block | exit |
|:---|:--:|
| `c1-unknown-field_example_0` | 1 |
| `c2-duplicate-claim-id_a_0` | 0 |
| `c2-duplicate-claim-id_b_0` | 0 |
| `c3-genre-forbids-kind_0001-decision_0` | 0 |
| `c4-dangling-cite_a_0` | 0 |
| `c5-prose-cites-mismatch_a_0` (`target-claim`) | 0 |
| `c5-prose-cites-mismatch_a_1` (`mismatched-claim`) | 0 |
| `golden_composition-model_0` | 0 |
| `golden_lock-file-schema_0` | 0 |
| `golden_system-overview_0` | 0 |
| `orphan-claim_a_0` | 0 |

Every `docket.ncl` (all eight fixtures) validated against
`contracts/docket.ncl` with exit code 0.

This confirms exactly the intended shape: **only** `c1-unknown-field`'s
block fails schema validation; every other fixture's claim blocks —
including the ones that fail C2–C5 or the two precondition checks — are
individually well-formed, so C1 does not leak into any fixture but its own.

**Not executed — no program exists yet to run them:** `C2` (id
uniqueness), `C3` (genre-kind enforcement), `C4` (citation resolution),
`C5` (prose/`cites` agreement), `orphan-claim`, and `duplicate-stem`. Their
correctness rests on the reasoning in the table above and the judgment
calls section, not on a machine-checked run.
