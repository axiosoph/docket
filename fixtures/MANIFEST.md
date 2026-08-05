# Fixture manifest

Each subdirectory of `fixtures/` is a **self-contained corpus root**: its
own `docket.ncl` plus a `docs/` tree, meant to be pointed at as one
invocation (`docket check fixtures/<name>/`). Corpus roots never share
state — nothing in one fixture directory can collide with another,
document identifiers included, since an identifier is a corpus-relative
path and corpus roots don't overlap.

The `run-*/` fixtures additionally carry a `src/` tree — the runner
(`docket run <id>`, `src/marker.rs`/`src/run.rs`) scans the *whole*
corpus root for `@docket: <id> :: <command>` markers, not only `docs/`,
so their evaluator markers live beside a stand-in Rust test the way a
real corpus's would. All seven still `docket check` cleanly (see their
own row): they isolate `run` behavior, not a `C`-check failure.

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
| `c2-duplicate-claim-id/` | `C2` | `docs/specs/a.md` and `docs/specs/b.md` both head a claim `[dup-id]`. Each claim block is independently well-formed, in a genre that permits its kind, with no `depends`/`because` — so nothing but the id collision fails. |
| `c3-genre-forbids-kind/` | `C3` | `docket.ncl` declares `docs/adr/**` with `kinds = []` (MVP.md §2's "no claims permitted" form). `docs/adr/0001-decision.md` declares a `requirement` claim there anyway. |
| `c4-dangling-cite/` | `C4` | `docs/specs/a.md`'s claim `depends: [nonexistent-claim]`, which resolves to nothing in the corpus. A prose link `[related work](nonexistent-claim)` with the identical target is present so `C5` (every `depends`/`because` entry has a matching prose link) still passes — see the C5-vs-C4 judgment call below. |
| `c5-prose-cites-mismatch/` | `C5` | `docs/specs/a.md` declares `[target-claim]` (cited, and it resolves — so `C4` passes) and `[mismatched-claim]`, which declares `depends: [target-claim]` but whose prose body carries no matching link. `[target-claim]` exists specifically so the citation resolves and `C4` cannot also fire. |
| `orphan-claim/` | `orphan-claim` | `docs/specs/a.md` is a claim block with no preceding heading in the file at all — the minimal case of "no `[id]`-shaped heading precedes it" (MVP.md §1.1). |
| `orphaned-because/` | `orphaned-because` | `docs/specs/a.md`'s claim declares `because: [nonexistent-claim]`, which resolves to nothing. A matching prose link is present so `C5` still passes. Exits **0** — `orphaned-because` is `Warn` severity, distinct from `C4`'s `Fail`; see below. |
| `bare-reference-no-failure/` | — (all pass) | Not a check fixture — see below. `docs/specs/a.md`'s claim declares neither `depends` nor `because`, but its prose links to a target that does not exist in the corpus. Proves the noise-suppression property: an undeclared (bare) reference is never resolved against the corpus at all. |
| `explanation-forbids-kinds/` | — (config error) | Not a `C`-check fixture — see below. Isolates the `quadrant`/`kinds` derived rule (MVP.md §2), a `docket.ncl` config error, not a corpus-content check. |
| `normative-prose-unclaimed/` | `normative-prose` | `docs/adr/0001-decision.md` (genre `docs/adr/**`, `kinds = []`) carries a bare `MUST` in its own prose, with no claim block anywhere in the file. See below. |
| `normative-prose-quoted/` | — (all pass) | Not a check fixture — see below. The identical keyword, present only inside a block quote and inside an inline code span, in the same `kinds = []` genre. |
| `blast-doc-anchor/` | — (all pass) | Not a check fixture — see below. Isolates a `blast` behaviour rather than a check. |
| `anchor-link-satisfies-claim-id/` | — (all pass) | Not a check fixture — see below. Isolates C5's anchor-form acceptance for a claim-id declaration. |
| `golden/` | — (all pass) | See below. |
| `run-pass/` | — (all pass; `docket run always-true` exits 0) | Not a `C`-check fixture — see below. `src/lib.rs` carries `// @docket: always-true :: true`; the marker's command exits 0. |
| `run-fail/` | — (all pass; `docket run always-false` exits 1) | Not a `C`-check fixture — see below. `src/lib.rs` carries `// @docket: always-false :: false`; the marker's command exits 1. |
| `run-absent/` | — (all pass; `docket run unbacked-claim` exits 3) | Not a `C`-check fixture — see below. `[unbacked-claim]` declares `evaluator: test`; no `@docket:` marker for it exists anywhere in the corpus. |
| `run-none/` | — (all pass; `docket run not-yet-implemented` exits 0) | Not a `C`-check fixture — see below. `[not-yet-implemented]` declares `evaluator: none`; a marker for it exists (`:: false`) but must never be consulted. |
| `run-vacuous-missing/` | — (all pass; `docket run missing-test-target` exits 4) | Not a `C`-check fixture — see below. The marker names a test that was renamed/deleted; its command still exits 0. |
| `run-vacuous-ignored/` | — (all pass; `docket run ignored-test-target` exits 4) | Not a `C`-check fixture — see below. The marker names a real `#[ignore]`d test; cargo collects and skips it, still exiting 0. |
| `run-vacuous-exempt/` | — (all pass; `docket run exempt-target` exits 0) | Not a `C`-check fixture — see below. The marker's `!` opts its command out of vacuity detection even though its output would otherwise match. |
| `run-multi-block-pass/` | — (all pass; `docket run multi-block-target` exits 0) | Not a `C`-check fixture — see below. Reproduces the real two-binary shape a `cargo test` invocation prints on a crate with both unit tests and doc-comment examples: a unittest `test result: ` block with three real passes, followed by a doctest block reading `0 passed; 0 failed`, the same shape `run-vacuous-missing/` isolates. Proves a block that checked nothing does not sink an invocation that has a sibling block with real activity. |
| `bold-form-definitions/` | — (all pass) | Not a check fixture — see below. Isolates the bold-form recognizer: five definitions — one direct-colon, two parenthetical (one non-ASCII), and two italicized revision notes (one multi-line) — each with a matching block. |
| `bold-form-false-positives/` | — (all pass) | Not a check fixture — see below. The false-positive floor: ordinary bold text, a mid-sentence citation, a line-start bracket with no adjacent punctuation, and a list-embedded bracket — none recognized as a definition. |
| `c2-duplicate-across-forms/` | `C2` | `docs/specs/a.md` declares `[dup-across-forms]` in heading form, `docs/specs/b.md` declares the same id in bold form. Proves a duplicate arising from two *different* recognizers is still one C2 finding pair, each naming the other's site. |
| `unregistered-definition/` | `unregistered-definition` (`Warn`) | See below. `docs/specs/a.md` carries one bold-form and one heading-form definition with no `claim` block, plus one registered heading-form definition for contrast. Exits **0** — `Warn` severity, the coverage count. |
| `malformed-id/` | `malformed-id` (`Warn`) | See below. `docs/specs/a.md` carries one bold-form and one heading-form definition whose id fails the kebab-case grammar (both the real-corpus shape: an otherwise-kebab id with one stray uppercase segment), a bracketed-but-multi-word false positive that must not fire, and one registered heading-form definition for contrast. Exits **0** — `Warn` severity, never blocking. |
| `unreachable-reference/` | `unreachable-reference` (`Fail`) | See below. `docs/specs/a.md`'s claim `[unreachable-target]` links `../../.scratch/notes.md` in prose; the fixture's own `.gitignore` marks `.scratch/` ignored. Exits **1**. |
| `signals-zero-inbound/` | — (all pass) | Not a check fixture — see below. Isolates `docket signals`: one claim with a citer, one that cites but is never cited itself, one that neither cites nor is cited. |
| `run-absence-pass/` | — (all pass; `docket run no-retry-header` exits 0) | Not a `C`-check fixture — see below. `[no-retry-header]` declares `evaluator: absent`; its marker's literal, `Retry-After`, does not occur anywhere in `src/lib.rs`. |
| `run-absence-fail/` | — (all pass; `docket run retry-header-returned` exits 1) | Not a `C`-check fixture — see below. Same shape as `run-absence-pass/`, except `src/lib.rs` contains the literal — the absence claim is broken. |
| `absent-marker-stale/` | `absent-marker-stale` (`Warn`) | See below. `[no-retry-header]`'s marker names `Retry-After`, but the claim's prose was rewritten to no longer mention it as a code span. Exits **0** — `Warn` severity, never blocking. |

**`duplicate-stem/` is retired**, not just its row here. MVP.md §1.3 was
amended once a real corpus produced three `README.md` files under one
genre (`docs/models/{lean,lean-surety,tla}/README.md`), indistinguishable
by basename: document identifiers are now corpus-relative paths, unique
by construction, so there is nothing left for a stem-collision check to
detect. The fixture directory and its two empty files are deleted.

**Every fixture's `docket.ncl` carries the required `quadrant` field**
(MVP.md §2), added to every genre without changing which check the
fixture isolates: `docs/specs/**`/`docs/models/**`/`docs/architecture/**`
genres are `quadrant = "reference"`, and `docs/adr/**` (`kinds = []`)
genres are `quadrant = "explanation"` — the same mapping MVP.md §2's own
worked example uses. Neither value interacts with any `C`-check or
`orphan-claim`, so no fixture's isolated failure changed.

## `explanation-forbids-kinds/`

Isolates the derived rule (MVP.md §2): a genre whose `quadrant` is
`explanation` and whose `kinds` is non-empty is a configuration error.
`docket.ncl` declares `docs/adr/**` with `kinds = ["requirement"]` and
`quadrant = "explanation"` — the exact contradiction the rule forbids.

This is **not** one of the five corpus checks (C1–C5) or `orphan-claim`:
it is caught at config-load time, before the corpus is ever scanned, the
same way an ambiguous genre pattern or an unknown `kind` value already
are. `docket check --corpus fixtures/explanation-forbids-kinds` exits
**2** (usage/configuration error, MVP.md §5), with a message naming the
offending genre and its `kinds`, rather than exiting 1 with a `Failure`
entry in the index.

**The rule is enforced only in Rust (`config::load_config`), not in
`contracts/docket.ncl`.** `nickel export --apply-contract
contracts/docket.ncl` on this fixture's `docket.ncl` exits **0** — the
Nickel contract validates the *shape* of `quadrant` and `kinds`
independently but does not cross-check them against each other, by
design (see `contracts/docket.ncl`'s comment on `Genre`). Only
`docket check` (or a direct call to `config::load_config`) observes the
failure. This fixture's row above is marked "config error" rather than
"all pass" for exactly that reason: the corpus content is fine, and would
pass every `C`-check if the config ever let it load.

`docs/adr/0001-decision.md` mirrors `c3-genre-forbids-kind/`'s decision
document — a `requirement` claim under an ADR-shaped path — kept for
realism as a self-contained corpus root, even though `docket check` never
reaches it: the config error is returned before any file is scanned.

## `normative-prose-unclaimed/` and `normative-prose-quoted/`

Isolate the `normative-prose` check: a genre whose `kinds` is empty
permits no claim blocks, which already means no RFC-2119 keyword may
appear in that genre's own-voice text either — the mechanism by which a
decision record is prevented from carrying normative content extends
from *claim blocks* (`kinds = []`, C3) to *unregistered prose* (this
check), since a bare `MUST` is a binding assertion whether or not it
carries a block.

Both fixtures share one genre — `docs/adr/**`, `kinds = []`,
`quadrant = "explanation"` — and neither carries a claim block at all;
the check fires independently of C3/orphan-claim, so nothing else in
either corpus is capable of failing.

- **`normative-prose-unclaimed/`** — `docs/adr/0001-decision.md` line 3:
  `This decision MUST be treated as final.`, a bare own-voice `MUST`.
  `docket check --corpus fixtures/normative-prose-unclaimed` exits **1**
  with exactly one `normative-prose` failure naming the file, the line,
  and the genre.
- **`normative-prose-quoted/`** — the identical keyword, present twice,
  neither instance in the document's own voice: once inside a block
  quote (`> ... MUST retry.`, quoting a rejected proposal so the
  document can refute it) and once inside an inline code span
  (`` `MUST` ``, naming the token rather than asserting it). This is the
  fixture that proves the *design* rather than the feature — a
  regex over raw text cannot tell a quoted `MUST` from an asserted one,
  and this corpus is built to make exactly that distinction load-bearing.
  `docket check --corpus fixtures/normative-prose-quoted` exits **0**.

## `orphaned-because/` and `bare-reference-no-failure/`

Isolate the reference-kinds severity split (MVP.md §1.2, §3): a `depends`
target and a `because` target carry different remedies when dangling, and
a reference declared as neither is not this tool's concern at all.

- **`orphaned-because/`** — `docs/specs/a.md`'s claim
  `[orphaned-because-claim]` declares `because: [nonexistent-claim]`, with
  a matching prose link (`[the old reason](nonexistent-claim)`, so C5
  cannot also fire). `docket check --corpus fixtures/orphaned-because`
  exits **0** — a `Warn`-severity diagnostic never flips the exit code —
  while still printing exactly one `orphaned-because` diagnostic on
  stderr, whose message says the *reason* is orphaned, not the claim.
  This is the fixture that would have exited 1 under a design that gave
  `depends` and `because` the same severity, which is exactly the design
  R1 (the reference-kinds requirements) rejects.
- **`bare-reference-no-failure/`** — `docs/specs/a.md`'s claim
  `[bare-ref-claim]` declares neither `depends` nor `because` at all; its
  prose links to `nonexistent-target`, which resolves to nothing in the
  corpus. `docket check --corpus fixtures/bare-reference-no-failure` exits
  **0** with **zero** diagnostics — not a warning, not a failure. This is
  the noise-suppression property: an undeclared prose link is a bare
  reference by construction (nothing marks it as one; the absence of a
  `depends`/`because` entry *is* the marking), so it is never resolved
  against the corpus, and a target's deletion produces no signal at all.
  Also proves C5's replaced rule is one-directional: an undeclared prose
  link is never required to correspond to a declared entry.

## `blast-doc-anchor/`

Isolates the defect fixed alongside `blast-semantics`: a `depends`/`because`
entry that is a document anchor (§1.3) must create a reverse edge, the
same as a claim-id entry does. Under the pre-fix implementation, this
corpus's blast query returned nothing; a maintainer relying on it would
have missed exactly the class of dependency §4.1's own worked example
depends on.

- `docs/models/rule.md` — hosts `## 3. The rule`, a section with no
  claim block of its own: the fixture isolates a citation *targeting a
  section*, not a claim living in one.
- `docs/specs/a.md` — claim `[depends-on-rule]` declares
  `depends: [docs/models/rule#3]`, the **doc-path#anchor** form, with a
  matching prose link.
- `docs/specs/b.md` — claim `[depends-transitively]` declares
  `depends: [depends-on-rule]` by claim id, with a matching prose link.
  This proves
  the walk does not dead-end at a document-anchor citer: `x` (found via
  the anchor) is still citable by its own claim id, exactly like any
  other claim.

`docket blast 'docs/models/rule#3' --corpus fixtures/blast-doc-anchor`
must report both `depends-on-rule` and `depends-transitively`, in that
order. The corpus passes `docket check` cleanly — this fixture
demonstrates a `blast` behaviour, not a check failure.

## `anchor-link-satisfies-claim-id/`

Isolates C5's anchor-form acceptance (MVP.md §1.3, "Which link forms
satisfy a declaration"): a prose link in the **anchor form** —
`[…](#target-claim)` — must satisfy a `depends`/`because` entry naming a
claim id, not only the bare-href form (`[…](target-claim)`, unlinkable
by any tool but this one). Before that rule existed, this exact corpus
failed C5: the anchor form normalized to a doc-anchor
(`docs/specs/a#target-claim`), which never matched the bare claim id
`target-claim` the `depends` entry carries.

- `docs/specs/a.md` — claim `[target-claim]`, no `depends`/`because` of
  its own, present only so the second claim's citation resolves (C4
  passes) — so nothing but the anchor-form C5 question is live.
- Same file, claim `[depends-on-target]` — declares
  `depends: [target-claim]` and links it as `[the target](#target-claim)`,
  the same-file anchor form (MVP.md §1.3, "Which link forms satisfy a
  declaration").

`docket check --corpus fixtures/anchor-link-satisfies-claim-id` exits
**0** with zero diagnostics. Removing the prose link (leaving the
`depends` entry undeclared-by-link) flips this to a C5 failure — see
`checks::tests::c5_still_fails_a_claim_id_depends_entry_with_no_prose_link_at_all`,
which covers that same-shape negative case directly rather than as a
second fixture.

## `run-pass/`, `run-fail/`, `run-absent/`, `run-none/`, `run-vacuous-missing/`, `run-vacuous-ignored/`, `run-vacuous-exempt/`, `run-absence-pass/`, `run-absence-fail/`

Isolate `docket run <claim-id>` (`src/marker.rs`, `src/run.rs`): given a
claim, execute the `@docket: <id> :: <command>` marker(s) that discharge
it and report `pass` / `fail` / `absent` / `none` / `vacuous` — the
three-outcome distinction the runner's own dispatch names as its most
easily lost property (`absent` must never read as `fail`), the fourth,
unconditional state `evaluator: none` gets without ever touching the
marker scan, and the fifth, added by a later dispatch closing a
green-by-construction hole: a marker's command can exit 0 while its own
output proves nothing was actually checked.

- **`run-pass/`** — `docs/specs/a.md` declares `[always-true]`
  (`evaluator: test`); `src/lib.rs` carries `// @docket: always-true ::
  true`. `docket run always-true --corpus fixtures/run-pass` prints
  `pass  always-true  test` plus the one marker's `ok` line, and exits
  **0**.
- **`run-fail/`** — `[always-false]`, marker `// @docket: always-false ::
  false`. `docket run always-false --corpus fixtures/run-fail` prints
  `fail  always-false  test` plus a `FAIL (exit 1)` line, and exits
  **1**. Watched red directly (not merely asserted): `false` always
  exits 1, so this is the actual failure path, not an assumed one.
- **`run-absent/`** — `[unbacked-claim]` (`evaluator: test`), and
  **no** `@docket:` marker for it anywhere in the corpus. `docket run
  unbacked-claim --corpus fixtures/run-absent` prints `absent
  unbacked-claim  test` and the literal "no marker found" line — never
  the `fail` label or a `FAIL (exit …)` line, which is exactly the
  distinction this fixture exists to pin. Exits **3**, distinct from
  both `fail`'s **1** and the usage-error **2** an unknown claim id
  gets.
- **`run-none/`** — `[not-yet-implemented]` (`evaluator: none`, under
  `docs/architecture/**`), *and* `src/lib.rs` carries a marker for it
  (`// @docket: not-yet-implemented :: false`) whose command would fail
  if run. `docket run not-yet-implemented --corpus fixtures/run-none`
  prints `none  not-yet-implemented  none` with no marker line at all,
  and exits **0** — proving the marker is never consulted, not merely
  that none happens to exist. Removing the marker changes nothing about
  this fixture's `run` output, which is the point: `none` needs nothing
  executable to be expressible.
- **`run-vacuous-missing/`** — `[missing-test-target]` (`evaluator:
  test`); `src/lib.rs`'s marker names a test that was renamed or
  deleted. Its command's exit status alone would read as `pass` (a
  filtered-to-nothing `cargo test -- --exact` run still exits 0); the
  marker's `printf` reproduces the exact, measured `cargo test` summary
  line for that case (`test result: ok. 0 passed; 0 failed; ...`), and
  `run.rs::detect_vacuity` recognizes it. `docket run
  missing-test-target --corpus fixtures/run-vacuous-missing` prints
  `vacuous  missing-test-target  test` plus a `VACUOUS (...)` marker
  line with the captured stdout, and exits **4** — distinct from every
  other outcome's code.
- **`run-vacuous-ignored/`** — `[ignored-test-target]`; the marker names
  a *real*, `#[ignore]`d test, a different shape from the fixture above
  (cargo does collect it, `running 1 test`, then marks it `ignored`) that
  still lands on the identical `0 passed; 0 failed` summary counts —
  proving one recognizer covers both cases rather than needing a second.
  `docket run ignored-test-target --corpus fixtures/run-vacuous-ignored`
  also prints `vacuous  ignored-test-target  test` and exits **4**.
- **`run-vacuous-exempt/`** — `[exempt-target]` (`evaluator: proof`, a
  stand-in for an evaluator kind — Lean/TLA+/Alloy — the runner has no
  output recognizer for at all); the marker's id carries a trailing `!`
  (`@docket: exempt-target! :: printf '...'`) whose command's output is
  deliberately built to match the same vacuity signal the two fixtures
  above trigger. `docket run exempt-target --corpus
  fixtures/run-vacuous-exempt` prints `pass  exempt-target  proof` and
  exits **0** — proving the exemption is read and actually bypasses
  detection, not merely that no signal happened to match.

- **`run-absence-pass/`** — `[no-retry-header]` (`evaluator: absent`,
  `src/absence.rs`); its marker, embedded in `docs/specs/a.md`'s own
  prose (the `<!--\n@docket: ... \n-->` three-line form MVP.md's "Run"
  section recommends — a single-line HTML comment would swallow the
  trailing `-->` into the literal, since the marker grammar already
  takes the rest of the line verbatim), names `Retry-After`; `src/lib.rs`
  never mentions it. `docket run no-retry-header --corpus
  fixtures/run-absence-pass` prints `pass  no-retry-header  absent` plus
  the marker's `ok` line, and exits **0** — the literal's absence,
  confirmed.
- **`run-absence-fail/`** — `[retry-header-returned]`, identical shape,
  except `src/lib.rs` contains the literal (a string, real code — the
  fixture's whole point). `docket run retry-header-returned --corpus
  fixtures/run-absence-fail` prints `fail  retry-header-returned  absent`
  plus a `FAIL (exit 1)` line naming exactly where the literal was found
  (`src/lib.rs:4`), and exits **1** — the same `fail` outcome and exit
  code an ordinary broken claim gets, not a sixth code (`run.rs`'s module
  docs, "Absence claims").

Both `run-absence-*` fixtures `docket check` cleanly at exit 0, same as
the other seven above — the `run` outcomes are a distinct code path
(`main.rs`'s `Command::Run`), never a `C`-check.

## `absent-marker-stale/`

Isolates `absent-marker-stale` (`src/absence.rs`'s `find_stale_markers`,
`register.ncl`): `[no-retry-header]`'s marker still names `Retry-After`,
but the surrounding prose was rewritten to no longer carry it as a code
span — the drift the check exists to catch
(`.ledger/2026-08-05-references-that-leave-the-register.md`, O3).
`docket check --corpus fixtures/absent-marker-stale` reports one `warn`
diagnostic naming the claim id and the stale literal, and exits **0** —
`Warn` severity, like `unregistered-definition`/`malformed-id`: this
asserts nothing about whether the underlying absence still holds (that
is `run`'s job, over source), only that the marker may no longer
describe anything the document currently says.

## `golden/`

One document per MVP.md §2's example genre (`docs/specs`, `docs/models`,
`docs/architecture`, `docs/adr`), reproducing the worked example from
MVP.md §4.1 / README.md's claim-block sample almost verbatim:

- `docs/specs/lock-file-schema.md` — claim `[lock-groundness]`
  (`kind: constraint`), declaring `depends: [docs/models/composition-model#6,
  docs/models/execution-model#2.4]` — the **doc-path#anchor** reference
  form, both instances also given matching prose links written as real
  relative markdown paths (`../models/composition-model.md#6`), which
  `checks::normalize_prose_link` resolves against this file's own
  directory to the same path-based ref the `depends` entry carries. Both
  are `depends`, not `because`: the claim's own text ("names bound to
  content identities and exact version strings") states what those two
  model sections *define*, so the claim's meaning requires them to exist.
- `docs/models/composition-model.md` — hosts the `## 6` heading
  `lock-groundness` depends on, plus its own claim `[atom-identity]`
  (`kind: invariant`, `evaluator: proof`, no `depends`/`because`) to
  exercise a second kind/evaluator pair.
- `docs/models/execution-model.md` — hosts the `## 2.4` heading
  `lock-groundness` depends on. No claim block of its own.
- `docs/architecture/system-overview.md` — claim `[system-boundary]`
  (`kind: requirement`, `evaluator: none`), declaring `because:
  [lock-groundness]` — the **claim id** reference form (unaffected by the
  path-refs change: claim ids are bare kebab-case tokens, never paths),
  with a matching prose link. `because`, not `depends`: deleting
  `lock-groundness` would not make the architectural fact ("the boundary
  sits between the atom store and the build graph") false, only
  under-justified — the reverse of `lock-groundness`'s own citations,
  deliberately, so `golden/` exercises both kinds.
- `docs/adr/0001-use-nickel.md` — a decision record. `kinds = []` for this
  genre, so it carries no claim block, which is the compliant state for
  that genre rather than an exception to it.

All five document paths are distinct, all three claim ids are distinct,
every `depends`/`because` entry matches a corresponding prose link, and
every citation resolves.

## Bold-form definitions and the coverage count

A second marking convention for a claim's id, alongside `### [id]`
(MVP.md §1.1's heading form): a bracket-kebab id wrapped in `**…**` at
the very start of a line, immediately followed by definitional
punctuation. A team-lead dispatch measured a real specification corpus
at 418 recognized definitions, only 18 in heading form — this is the
other 400.

**Four properties, all required**, extraction.rs's `definitional_punctuation_len`
doc comment states the exact grammar:

1. **Line start** — the byte immediately before the opening `**` is a
   newline or the start of the file. This is checked directly against
   the source, not inferred from tree nesting, and it has a useful side
   effect: a block quote (`> **[id]**…`) and a list item
   (`- **[id]**…`) are *both* excluded automatically, since a `>` or a
   list marker always sits between the line start and the `**` — no
   separate guard against either was needed.
2. **The `**…**` wrapper.**
3. **A bracketed kebab id inside it**, reusing `bracket_kebab_id`
   unchanged — the same grammar the heading form already enforces.
4. **Definitional punctuation immediately after**, with no intervening
   prose: a direct colon (`**[id]**: …`, 383/400 in the measured
   corpus), or a parenthetical then a colon (`**[id]** (P8): …`, the
   other 17 — the parenthetical's content is unconstrained, since only
   its own `)` bounds it, so it may hold non-ASCII like `P9′`).

**Block placement.** A bold-form definition sits inline in prose, so a
fenced block cannot follow it directly the way a heading-form block
can. The rule adopted: a claim block is owned by the **nearest
preceding recognized definition, either form** — the exact rule the
heading form already used (a deeper heading wins over a shallower one,
scanning back across every earlier heading regardless of level),
generalized from one shape to two rather than replaced. A human author
writes the id, elaborates in prose, and eventually reaches the block;
nothing here requires the block to be adjacent, only nearest. This
answers the dispatch's own question without a genuine ambiguity to
halt on: the ownership rule was already form-agnostic in spirit (it
never depended on "heading" specifically, only on "nearest preceding
recognized id"), so extending it to a second form is not a new
convention, only a wider one.

A bold-form definition's own **prose scope** (C5, §3) is narrower than
a heading's, deliberately: it ends at the earliest of the next
definition (either form) or the next heading of *any* level — unlike a
heading-form definition, whose scope survives a deeper subheading
(MVP.md §1.1's `nested_headings_find_the_nearest_bracket_kebab_ancestor`
case). A bold-form definition is a sentence inside a section, not a
section of its own, so nothing beneath the next heading — even a
subheading — belongs to it.

**The coverage count** (dispatch's second deliverable): a recognized
definition, either form, that no claim block ever adopted. Reported as
a new diagnostic, `unregistered-definition` (`Warn` severity, never
flips the exit code) rather than a bespoke subcommand or output format:
the `(file, line, id)` shape a definition site needs is exactly what
`Diagnostic` already carries, corpus-loading and genre-scoping are
already `run_checks`'s job, and a corpus with hundreds of these is the
normal starting state (the dispatch's own words) — never a reason to
add a second code path that has to agree with the first about which
files were scanned.

- **`bold-form-definitions/`** — the positive case: all five
  syntactic variants (direct colon; parenthetical; parenthetical with a
  non-ASCII prime; an italicized revision note; a multi-line italicized
  revision note with an em dash), each with a matching block. The italic
  pair is a later generalization: a real-corpus measurement found the
  original two-form recognizer silently invisible to exactly the
  definitions carrying revision history (`_(amended …)_`,
  `_(retired …)_`, `_(superseded …)_`, `_(disambiguated …)_`), so this
  form is not a third enumerated case but the same "bounded, colon-free
  interstitial" rule the direct and parenthetical forms already satisfy,
  applied to a markup-wrapped span instead of a plain one. `docket check
  --corpus fixtures/bold-form-definitions` exits **0**.
- **`bold-form-false-positives/`** — the false-positive floor
  (criterion 4): ordinary bold text with no bracket-kebab id; a
  mid-sentence citation of a real id (bracketed, but not line start);
  a line-start bracket with no adjacent punctuation; and a
  list-embedded bracket (not line start, for the same reason a block
  quote isn't). None is recognized as a definition — the corpus's one
  real definition is the only claim, and the check emits zero
  diagnostics. Exits **0**.
- **`c2-duplicate-across-forms/`** — criterion 5: `[dup-across-forms]`
  declared once in heading form (`docs/specs/a.md`) and once in bold
  form (`docs/specs/b.md`). C2 groups by `Claim::id` alone, so this
  needed no new logic — the fixture is proof the existing rule already
  covers a duplicate arriving via two different recognizers, reported
  as two diagnostics, each naming the other's site. Exits **1**.
- **`unregistered-definition/`** — the coverage count itself: one
  bold-form and one heading-form definition with no block, plus one
  registered heading-form definition for contrast (silent, as any
  registered claim already is). `docket check
  --corpus fixtures/unregistered-definition` exits **0** with exactly
  two `unregistered-definition` warnings on stderr, naming the file,
  line, and id of each.

## `malformed-id/`

`.ledger/2026-08-04-malformed-ids-are-silently-invisible.md`: a bracketed
token that satisfies every *structural* property of a definition — line
start, the wrapper, immediately-following punctuation for bold form; the
whole heading text for heading form — but whose inner content fails the
lowercase-kebab grammar was previously invisible to every check at once:
not a claim, not `unregistered-definition`, not anything. Reported now as
`malformed-id` (`Warn` severity, same treatment as
`unregistered-definition`), under its own diagnostic because the remedy
differs from `unregistered-definition`'s: rename the id, not write a
claim block.

`docs/specs/a.md` carries four definitions:

- `**[boundary-L1-concerns]**` (bold form) and `### [Daemon-Discovery]`
  (heading form) — both structurally complete definitions whose id
  carries an uppercase segment, the exact real-corpus shape the ledger
  entry measured (`boundary-L1-concerns` through `boundary-L5-concerns`,
  `daemon-discovery-vN`). Neither is a claim, and neither ever could be
  a valid claim id — `malformed-id` is the only diagnostic naming them.
- `**[Note to reader]**` — the false-positive floor: a bracket whose
  inner content carries whitespace reads as an ordinary sentence, not an
  attempted id, and must produce **no** diagnostic at all (not
  `malformed-id`, not `unregistered-definition`).
- `### [registered-claim]` — a normal, well-formed, registered
  definition, present only for contrast: it stays completely silent, the
  same as any other passing claim.

`docket check --corpus fixtures/malformed-id` exits **0** with exactly
two `malformed-id` warnings on stderr, naming the file, line, and
offending id of each — never touching the index (`registered-claim` is
the only entry) and never producing `unregistered-definition` for the
two malformed sites, since a malformed id was never a recognized
definition to begin with.

## `unreachable-reference/`

`.ledger/2026-08-05-references-that-leave-the-register.md`, O4: a
reference whose target *exists* but sits somewhere the reader cannot go
— a gitignored working directory, distinct from a dangling reference
(`C4`, target absent entirely). `Fail` severity, unlike
`unregistered-definition`/`malformed-id`: an unreachable reference has no
grace period the way an unregistered definition does (a real corpus is
not expected to carry any on the day this check ships), and the head's
own ruling calls the underlying rule "not legal," the same weight C4's
"this claim is broken" carries.

`docs/specs/a.md`'s claim `[unreachable-target]` links
`../../.scratch/notes.md` in its prose, resolving (relative to the citing
file's own directory) to `.scratch/notes.md`; the fixture's own
`.gitignore` marks `.scratch/` ignored, so `git check-ignore` reports it.
The claim declares neither `depends` nor `because`, so nothing else in
the corpus is capable of firing — `docket check --corpus
fixtures/unreachable-reference` exits **1** with exactly one
`unreachable-reference` failure naming the file, the line, the link as
written, and the corpus-relative path it resolved to.

**Scope boundary: "path-shaped."** Not every link is checked — a bare
word with no `/`, no `.`, and no `#anchor` (e.g. `[see also](sibling-claim)`)
reads as a claim-id citation, the same distinction `register.ncl`'s own
`normalize_prose_link` already draws for C5, and is never resolved
against `git` at all. A claim id is lowercase-kebab by grammar and can
therefore never contain a `.`, which makes that split exact rather than
a guess: `gitignore::tests::a_claim_id_shaped_bare_word_is_never_checked_even_if_it_would_match`
pins a case where the bare word *is* a real, gitignored directory name
and the check still stays silent, proving the boundary is applied before
`git` is ever asked rather than merely never triggering it by
coincidence. An href carrying a `#anchor` is always path-shaped
regardless of slashes or dots — `model.rs`'s own `CiteRef` grammar never
gives a claim id an anchor, so the presence of one is unambiguous.

**Directional, not merely filtered.** The reverse — an ignored file
linking into the repository — is legitimate ("only the author has
that," per the head's ruling) and is not merely unchecked by a
condition in `gitignore.rs`; it is structurally unreachable to this
check, because `corpus::load_corpus`'s walk never visits a dotdir at
all (`corpus::tests::skips_dotfiles_and_dotdirs`, unchanged by this
feature). `checks::tests::a_reference_into_the_repository_from_outside_it_is_never_this_checks_concern`
pins this directly: a gitignored document linking back into the corpus
produces no diagnostic of any kind, because it is never scanned to
begin with, not because its link happens to resolve to a tracked path.

**Determining ignored-ness: `git check-ignore --stdin`, batched once per
corpus.** Chosen over reimplementing `.gitignore` pattern matching for
the same reason MVP.md §7 gives Nickel authority over its own contract
language: git's own ignore semantics (nested `.gitignore` files, global
excludes, `.git/info/exclude`) are exactly what determines whether a
fresh clone would contain a path, and this check's whole point is
answering that question, not approximating it. Degrades to **silence** —
never a crash, never a false positive — when `corpus_root` is not inside
a git working tree (exit 128) or `git` itself is not on `PATH` (a spawn
error): a corpus with no git history to ask has no reader-reachability
question this check can answer at all
(`gitignore::tests::a_corpus_root_that_is_not_a_git_repository_degrades_to_silence`,
`gitignore::tests::a_missing_git_binary_degrades_to_silence_rather_than_a_crash`,
`checks::tests::a_corpus_that_is_not_a_git_repository_never_fires_unreachable_reference`).

## `signals-zero-inbound/`

Isolates `docket signals` (MVP.md §4.4): the derived-degree report over
the same reference graph `blast` already walks, headlined by zero-inbound
claims — a candidate list for superseded-and-unnoticed, never a verdict.

`docs/specs/a.md` carries three claims, chosen so out-degree and in-degree
read as genuinely independent axes rather than one number seen twice:

- `[base]` — no `depends`/`because` of its own, cited by `derived`.
  out-degree 0, in-degree 1, review-surface 1 (`derived` is its whole
  blast radius).
- `[derived]` — `depends: [base]`, with a matching same-file anchor-form
  prose link (`[base](#base)`, so C5 stays clean); nothing in the corpus
  cites `derived` itself. out-degree 1, in-degree **0**, review-surface 0
  — the case that motivates the fixture: a claim with real outgoing
  dependencies can still be zero-inbound, because in-degree counts who
  cites *it*, not what it cites.
- `[standalone-invariant]` — neither `depends` nor `because`, and nothing
  cites it either. out-degree 0, in-degree 0, review-surface 0 — the
  self-contained-leaf shape the report's own framing calls out: a
  legitimate zero-inbound claim, not a defect to fix.

`docket signals --corpus fixtures/signals-zero-inbound` lists `derived`
and `standalone-invariant` under zero-inbound (2 of 3 claims) and prints
all three rows in the per-claim table above with exactly the
out-degree/in-degree/review-surface values named above. The corpus passes
`docket check` cleanly — like `blast-doc-anchor/`, this fixture
demonstrates a query's behavior, not a check failure.

## Judgment calls made while writing these fixtures

MVP.md was read as the binding contract per the dispatch; nothing here
required a halt, but four points were ambiguous enough that a call had
to be made. Flagging them here rather than deciding silently — three of
the four are now resolved in the spec itself, marked below rather than
removed, since the resolution is part of the record.

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

2. **RESOLVED, now pinned in spec.** MVP.md §3 originally said C5's `L`
   was "restricted to targets that resolve"; this fixture set's own
   reasoning — that a resolution filter makes a dangling `cites` entry
   with a matching prose link fail both C4 and C5, with C5's message
   lying about a divergence that doesn't exist — was accepted, and §3 now
   reads "**ref-shaped**... a syntactic filter, *not* a resolution
   filter" with that exact rationale boxed in. `c4-dangling-cite/`
   isolates C4 exactly as originally designed.

3. **RESOLVED, superseded by MVP.md §1.3's prose-link normalization
   rule.** The bare-literal convention this fixture set originally used
   (`[text](composition-model#6)`, matching a `cites` entry
   character-for-character) is no longer how `golden/` is written. The
   spec now requires a prose href to be **resolved against the citing
   file's own directory** — an ordinary relative markdown link, written
   the way an author actually would (`[text](../models/composition-model.md#6)`)
   — before comparison. `golden/docs/specs/lock-file-schema.md` was
   rewritten to that form; the claim-id reference form
   (`[text](lock-groundness)` in `system-overview.md`) is untouched,
   since claim ids were never path-shaped and this rule doesn't touch
   them.

4. **RESOLVED, now pinned in spec.** MVP.md §1.3's "Anchor derivation"
   states the exact prefix-match rule this fixture set assumed (`## 6` →
   anchor `6`), with the rationale that section numbering churns less
   than heading wording. `golden/`'s headings needed no changes for this.

5. **Reference kinds (`depends`/`because`) retired `cites` corpus-wide.**
   Every fixture that declared `cites` (`blast-doc-anchor/`,
   `c4-dangling-cite/`, `c5-prose-cites-mismatch/`, `golden/`) was
   retyped rather than left on the old field, since `cites` is now an
   unknown field and would fail C1 in every one of them — leaving even
   one on the old name would have been a silent regression the migration
   itself was supposed to prevent. `golden/`'s two citations were
   deliberately typed to different kinds (`depends` in
   `lock-file-schema.md`, `because` in `system-overview.md`, both argued
   inline) so the fixture set exercises both, not just one by default.
   Two new fixtures (`orphaned-because/`, `bare-reference-no-failure/`)
   isolate the properties no existing fixture could: the `Warn`-severity
   split, and the noise-suppression guarantee that a bare reference is
   never resolved against the corpus at all.

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
| `explanation-forbids-kinds_0001-decision_0` | 0 |
| `golden_composition-model_0` | 0 |
| `golden_lock-file-schema_0` | 0 |
| `golden_system-overview_0` | 0 |
| `orphan-claim_a_0` | 0 |
| `orphaned-because_a_0` | 0 |
| `bare-reference-no-failure_a_0` | 0 |

`explanation-forbids-kinds`'s own claim block validates cleanly at C1 —
the fixture's failure is entirely in `docket.ncl`, before any block is
ever extracted; see its own section below.

Every `docket.ncl` across all thirteen fixture directories (reference
kinds' `orphaned-because/` and `bare-reference-no-failure/` included, and
`normative-prose-unclaimed/`/`normative-prose-quoted/` from before them)
validated against `contracts/docket.ncl` with exit code 0 —
`explanation-forbids-kinds/` included, since the Nickel contract does not
enforce the derived rule (see its own section below for why).

This confirms exactly the intended shape: **only** `c1-unknown-field`'s
block fails schema validation; every other fixture's claim blocks —
including the ones that fail C2–C5 or the two precondition checks — are
individually well-formed, so C1 does not leak into any fixture but its own.

**Stale note removed.** This section originally said C2–C5, `orphan-claim`,
and `duplicate-stem` had never been run because no program existed yet;
`docket` now implements all of them, `duplicate-stem` is retired, and
every fixture (plus `golden/`) is verified directly against the real
binary as part of each dispatch that touches this manifest — see the
commit history for the actual exit codes, which is the durable record
rather than a table frozen at fixture-authoring time.
