# MVP specification

The buildable contract for docket's first version. Scope is fixed by
[README.md](README.md) §MVP scope; this document states it precisely
enough to implement against without further design decisions.

**If something here is ambiguous, halt and ask rather than deciding.** A
gap filled by invention is worse than a gap reported.

---

## 1. The claim block

### 1.1 Syntax

A claim block is a fenced code block whose info string is exactly
`claim`, appearing in a markdown file. Its content is YAML.

````markdown
```claim
kind: constraint
evaluator: property-test
depends: [docs/models/composition-model#6, docs/models/execution-model#2.4]
```
````

**The claim's `id` is not in the block.** It is taken from the nearest
preceding **definition** — a marker elsewhere in the document whose
sole job is to state the id. Two forms are recognized:

**Heading form.** A heading whose text is exactly a bracketed
kebab-case token:

```markdown
### [lock-groundness]
```

**Bold form.** A `**…**` span, at the very start of a line, wrapping
exactly a bracketed kebab-case token, immediately followed by
definitional punctuation: a colon, or a single inline aside — a
parenthetical, an italicized note, or any other balanced inline markup
span — then a colon. The aside's own content is unconstrained (a formula
label, a revision note, even a bracketed id cited in passing) and may
itself contain a colon; what closes the definition is the first colon
that is not inside the aside. The aside is bounded to 128 bytes and may
not cross a line break — "immediately followed" does not survive
crossing one.

```markdown
**[lock-groundness]**: Every lock value MUST be ground.

**[lock-groundness]** (P8): Every lock value MUST be ground.

**[lock-groundness]** _(amended 2026-07-14 — retitled from
lock-nonzero)_: Every lock value MUST be ground.
```

Both forms exist because corpora do: a specification-first corpus
built around numbered sections tends toward the heading form, while a
prose-first corpus stating one requirement per paragraph tends toward
the bold form — and a real corpus measured against an early
heading-only draft of this tool had 400 of its 418 definitions in bold
form. **The tool learns the corpus's convention rather than requiring
the corpus to restructure around the tool's.** Recognizing a form is
deliberately permissive rather than requiring a canonical one: id
uniqueness (C2) adjudicates precision centrally, so an over-matching
recognizer produces a loud, corpus-wide duplicate-id failure — naming
every site that declared the id, regardless of which recognizer found
each one — rather than a silently invented claim. This is what keeps
adding a third form, later, cheap: each recognizer needs to be roughly
right, not perfect.

Rationale, unchanged by the addition of a second form: the id already
exists in prose as the human-readable anchor, and duplicating it into
the block would create precisely the divergence surface this tool
exists to remove. One statement, one place.

**Ownership.** A claim block belongs to the **nearest preceding
definition, either form** — one rule across both, not two: exactly the
existing "a deeper heading wins over a shallower one" behavior,
generalized from one shape to two rather than replaced. The block need
not be adjacent to its definition, only nearest to it — a bold-form
definition sits inline in prose, so a fenced block cannot follow it
directly the way it can a heading, and a human author reaches the
block after elaborating, not before.

A claim block with no such preceding definition anywhere in the file is
an error (`orphan-claim`).

**Unregistered definitions.** A recognized definition — either form —
with no claim block is *unregistered*: real corpus content the
register does not yet cover. Reported as `unregistered-definition`
(`Warn` severity, §5) rather than failed on, since a corpus is expected
to carry many of these on the day a registration effort begins; the
count is exactly the number that effort exists to move.

**Malformed ids.** A bracketed token that sits in definition position —
line-start `**[...]**` immediately followed by definitional punctuation,
or a heading whose entire text is `[...]` — but whose inner content is
not lowercase kebab-case is not recognized as a definition at all: it
becomes neither a claim nor an `unregistered-definition`, which means it
would otherwise produce **no diagnostic whatsoever** — the same silence
a correctly handled definition produces. Reported instead as
`malformed-id` (`Warn` severity, §5), under its own diagnostic rather
than folded into `unregistered-definition`: the remedies differ (rename
the id, versus write a claim block), and a malformed id is more likely a
mistake than a coincidence — prose rarely opens a line with a bolded
bracketed kebab-ish token followed by a colon. The id itself is never
normalized or auto-corrected; the grammar stays exactly what §1.1
already states, only violations of it become visible.

A bracket whose inner content carries whitespace is not treated as a
malformed id — it reads as an ordinary sentence (`**[Note to
reader]**: ...`), not an attempted identifier, so it produces no
diagnostic of any kind.

### 1.2 Fields

| field | required | type |
|:---|:--|:---|
| `kind` | yes | one of `requirement`, `invariant`, `constraint` |
| `evaluator` | yes | one of `proof`, `model-check`, `type`, `property-test`, `test`, `example`, `review`, `none` |
| `depends` | no (default `[]`) | array of refs |
| `because` | no (default `[]`) | array of refs |

Unknown fields are an error, not ignored. Tolerating them would let a
typo'd field name silently carry no meaning.

#### Reference kinds

A reference entry is not one undifferentiated `cites`. It names *why* a
claim points somewhere, because deleting the target means something
different depending on which:

| kind | meaning | on deletion of the target |
|:---|:---|:---|
| `depends` | the claim's truth or meaning requires the target | the claim is **broken** — rewire or remove |
| `because` | the claim's justification is the target | the claim **still stands**, under-justified — restate the reason, or discover it was vestigial |
| bare reference | context; asserts no dependence | nothing |

Only `depends` and `because` are fields on the block. A **bare** reference
has no field of its own — it is a prose link declared as neither, the
complement rather than a third array to populate. A field for it would let
an author *assert* bareness, which is incoherent: bareness is the absence
of a declaration, not a declaration of absence.

**The third kind is not optional.** Without it, authors overload one field
for both context and dependence, and every deletion either manufactures
false breakage (an incidental mention treated as load-bearing) or, worse,
teaches authors to under-declare to avoid the noise — and a register
nobody trusts is worse than none, because its silence reads as safety.

The closest analogue is a runtime versus a build dependency: a missing
runtime dependency means the artifact doesn't work; a missing build
dependency means it still works but can no longer be *derived*. That maps
the remedies exactly — the test of an analogy is that the consequences
match, not merely the words.

**These declarations are not verifiable.** Nobody can prove a claim really
depends on a target rather than merely mentioning it; a `depends` written
where a `because` belongs, or the reverse, is a review finding, not
something this tool can catch. The register exists to **bound the review
surface, not eliminate it** — which is the argument for keeping the
vocabulary exactly this small (two declared kinds, not a weighted or
graded dependence) rather than trying to be clever about intent.

### 1.3 Reference syntax

A `depends`/`because` entry is either:

- **a claim id** — kebab-case, resolving to a claim elsewhere in the
  corpus: `lock-groundness`
- **a document anchor** — `<doc-path>#<anchor>`, resolving to a heading in
  a corpus document, where `<doc-path>` is the **corpus-relative path with
  the `.md` extension removed**: `docs/models/composition-model#6`

**Paths, not basenames — and this retires a check.** An earlier draft used
the file basename, which the second real-corpus run refuted immediately: a
corpus had three `README.md` files inside one genre
(`docs/models/{lean,lean-surety,tla}/README.md`), indistinguishable by
basename. Requiring a corpus not to put `README.md` in subdirectories is
not a rule anyone can follow.

Using the path instead makes document identifiers **unique by
construction**, which means the `duplicate-stem` check has nothing left to
detect and is **retired**, along with its fixture. Paths contain `/`, which
the ref contract already accepts — no contract change is needed.

The cost is longer citations. The compensation is that a ref now shows its
genre, so `docs/models/composition-model#6` states on its face that it
points at a model.

**Ref syntax is enforced by the contract, not by C4.** A `depends`/`because`
entry whose *shape* is invalid — whitespace, a second `#`, an empty half —
is a malformed block and fails **C1**. C4 therefore only ever operates on
well-formed refs, and "does this resolve" presupposes "is this a ref."

**Prose-link normalization.** A markdown href in prose is normalized to a
ref before comparison (C5): **resolve it against the containing file's
directory** to get a corpus-relative path, strip the `.md` extension, retain
any `#fragment`, and treat a fragment-only href as referring to the
containing document. So `[…](../models/composition-model.md#6)`, written in
a file under `docs/specs/`, normalizes to `docs/models/composition-model#6`.

This is load-bearing rather than cosmetic: **real corpora link by relative
path with an extension**, and without normalization those links would never
match a `depends`/`because` entry, making C5 fire on every correctly-linked
claim.

**A leading `/` is corpus-root-relative**, not joined onto the citing file's
directory. So `[…](/docs/models/composition-model.md#6)` normalizes to
`docs/models/composition-model#6` from any file in the corpus. This follows
rendered-markdown convention, where a leading slash is site-root-relative, and
the alternative is incoherent: joining `/docs/...` onto a containing directory
produces a path that names nothing.

Note the normalization *resolves* the path rather than discarding it, which
is what makes it agree with §1.3's path-based refs — and it means an
ordinary relative markdown link, written the way an author would write it
anyway, normalizes to exactly the ref a `depends`/`because` entry carries.
A link that escapes the corpus root is not ref-shaped and is ignored.

**Anchor derivation.** An anchor `A` matches a heading iff the heading's
text — after stripping `#` markers and leading whitespace — begins with
`A` followed by either end-of-string or a non-alphanumeric character. So
`docs/models/composition-model#6` matches `## 6. The fact-set: …` and
`…/execution-model#2.4` matches `### 2.4 Something`, while `#6` does **not**
match `## 60. …`.

Section numbers rather than slugified heading text, deliberately: heading
*wording* churns far more often than section *numbering* in the corpora
this tool targets, so numbers are the more stable anchor. The failure mode
when a document is renumbered is loud — C4 fails immediately — rather than
silent.

**Which link forms satisfy a declaration (C5).** A `depends`/`because`
entry naming a **document anchor** is satisfied only by an exact match —
its normalized path and anchor must equal the entry's. An entry naming a
**claim id**, though, is satisfied by either of two prose-link forms:

- the **bare id** as the href, `[…](spine-chain-complete)` — resolved only
  by this tool's normalization, since it is neither a path nor a fragment;
- the **anchor form**, `[…](#spine-chain-complete)` or
  `[…](docs/x.md#spine-chain-complete)` — any normalized document anchor
  whose anchor component equals the id, independent of which document it
  names.

Both are accepted, non-exclusively: a documentation corpus must stay
readable by ordinary tooling, and the bare form is not that — no
markdown renderer resolves it to anything and no link checker accepts an
href that names no file, so a corpus that used it exclusively would ship
links this tool alone can follow. The anchor form is what a renderer
resolves and a link checker accepts, and what an author writes
unprompted, so it is required to work. The bare form stays *accepted*
rather than retired: retiring it would be a breaking migration for no
correctness gain, and this document's own claims (§4.1, §4.2 below) use
it already.

Accepting only one form is not a smaller version of this rule; it is a
different, broken one. A corpus restricted to the bare form ships links
no renderer or checker accepts, which a documentation corpus cannot
tolerate. A corpus restricted to the anchor form fares no better in the
other direction: the anchor form normalizes to a *document* anchor
(`<doc-path>#<id>`), which does not equal a bare claim-id declaration
under exact-target matching, so every claim-id reference written the way
an author and a link checker both expect would fail this check. A real
corpus that hit exactly this — required to choose one form, unable to
satisfy both this check and its own link-checking gate with either —
registered claims for eight references and kept zero edges in the
resulting graph, dropping every declaration rather than break either
gate. Accepting both is what makes a claim-id declaration reachable
regardless of which of the two legitimate authoring styles a writer
reaches for.

#### [reference-syntax]

A `depends`/`because` entry is a claim id or a `<doc-path>#<anchor>`
document anchor; document identifiers are the corpus-relative path with
`.md` removed, not a basename; a prose link normalizes to the same
vocabulary by resolving against the citing file's directory before
comparison; an anchor matches a heading by non-alphanumeric-bounded
prefix, not exact text.

```claim
kind: constraint
evaluator: test
```

## 2. Configuration

A repository declares its own genre hierarchy in `docket.ncl` at the
repository root. Genres are **not** built in.

**A path matching more than one genre is a configuration error (exit 2),
not a precedence question.** No first-match, no most-specific-wins. Both
conventions are defensible and both surprise somebody — and note that
first-match is the *opposite* of `.gitignore`, where later patterns
override — so rather than pick an order and document it, the config is
required to be unambiguous. Making patterns disjoint is trivial; silently
assigning a document to the wrong genre is exactly the failure this tool
exists to prevent.

```nickel
{
  genres = [
    { path = "docs/specs/**",        kinds = ["constraint"],  quadrant = "reference" },
    { path = "docs/models/**",       kinds = ["invariant"],   quadrant = "reference" },
    { path = "docs/architecture/**", kinds = ["requirement"], quadrant = "reference" },
    { path = "docs/adr/**",          kinds = [],              quadrant = "explanation" },  # no claims permitted
  ],
  # Files matching no genre are not scanned.
}
```

`kinds = []` means the genre may hold **no** claim blocks — the mechanism
by which a decision record is prevented from carrying normative content.
(§3's `normative-prose` check extends this from claim blocks to
unregistered RFC-2119 prose in the same genre.)

**`quadrant` names which of Divio's four documentation quadrants the
genre serves** — one of `tutorial`, `how-to`, `reference`, `explanation`.
It is a third, required field on every genre, closed over exactly those
four values.

A repository's own genre names do not travel: `docs/specs/**` means
nothing outside the repository that chose it. `quadrant` does, because
it names what job the genre does rather than where its files live — so it
is what makes "which quadrant has no claims?" a question comparable
across corpora, rather than one only answerable from inside a single
project's own conventions.

`quadrant` is **required**, not optional. An optional field would make
the gap analysis silently incomplete: a corpus with three unlabelled
genres would report three empty quadrants and read as a documentation
gap, when the actual gap is in the config.

**The derived rule: a genre whose `quadrant` is `explanation` and whose
`kinds` is non-empty is a configuration error (exit 2).** A claim is a
checkable assertion; explanation's job is rationale, not assertions to
check — so the two are a contradiction in the genre's own declaration,
not a corpus-content failure to discover by scanning documents. This
makes an existing convention a consequence instead of a rule argued case
by case: today a decision-record genre gets `kinds = []` because everyone
agrees a decision is terminal justification (README.md's genre table);
under quadrants, that follows from decision records being explanation.

#### [explanation-forbids-kinds]

A genre whose `quadrant` is `explanation` and whose `kinds` is non-empty
is a configuration error (exit 2): explanation carries rationale, and a
checkable assertion inside it is a genre violation by construction, not a
matter of convention.

```claim
kind: constraint
evaluator: test
```

**Only files with a `.md` extension are scanned.** Non-markdown files
inside a matched genre are skipped silently — not an error, not a warning.

A claim block can only live in markdown, so reading anything else is
wasted work at best. It is also a real failure mode rather than a
hypothetical: the first run against a real corpus aborted on a TLC
model-checker state dump (binary, no extension) sitting in a generated
subtree beneath a matched genre. That a model checker writes output under
`docs/models/` is a **repository-layout fact, not a misconfiguration**, and
every real corpus has some equivalent. Narrowing the genre pattern to dodge
it would push the tool's problem onto every config that uses it.

Extension-based rather than content-sniffed, deliberately: a content check
would have to open every file — the cost being avoided — and would make
"is this markdown?" a heuristic where an extension is a fact.

## 3. Checks

All five run on every invocation. Each failure names the file, the line,
and the offending value.

| id | check | failure means |
|:---|:---|:---|
| `C1` | every claim block validates against the contract | malformed or unknown field |
| `C2` | claim ids are unique corpus-wide | two homes for one id |
| `C3` | **`kind` ∈ the genre's permitted kinds for that path** | **a genre violation — a fact in the wrong home** |
| `C4` | every `depends` target resolves | **the claim is broken** — rewire or remove |
| `C5` | every `depends`/`because` entry has a matching prose link | a declared reference with no prose trail |

**`depends` and `because` are checked separately, at different severity**
(§1.2's reference kinds). C4 above covers only `depends`: a dangling
target means the claim itself is broken. A dangling `because` target is
`orphaned-because`, below — a distinct check at a strictly lower severity,
because the diagnosis is different in *kind*, not merely in degree: the
claim still holds, only its stated reason no longer resolves.

**C5, precisely — replaced.** The original rule required `L` — the set of
markdown link targets in a claim's prose body (from its id heading to the
next heading of the same or higher level, excluding the claim block
itself), restricted to targets that are **ref-shaped** per §1.3, a
syntactic filter, *not* a resolution filter — and `cites` to be equal as
sets. Reference kinds retire that equality: a bare reference is
legitimate and undeclared *by design* (§1.2), so requiring every prose
link to appear in a declared field would force every incidental mention
to be declared, collapsing context back into dependence — the exact
failure the third kind (§1.2) exists to prevent.

The replacement:

> **Every `depends` and `because` entry carries a prose link. A prose
> link that is not declared is a bare reference.**

Formally: let `D` be a claim's `depends` ∪ `because` entries and `L` its
normalized prose-link targets (which array an entry came from does not
matter here). A document-anchor entry in `D` matches `L` by exact target
equality; a claim-id entry matches by target equality **or** by any `L`
entry whose anchor component equals the id (§1.3, "Which link forms
satisfy a declaration") — the anchor form has no other representation,
since a claim id carries no anchor of its own. The rule is `D ⊆ L` under
that matching, not `D = L`. Everything declared must be linked, so a
reader following prose reaches what the graph says matters; nothing
requires the reverse, so an incidental mention costs nothing to leave
undeclared, and nothing is hidden — undeclared *means* bare, and bare
asserts nothing.

**This also relocates a job C5 was never able to do.** No formulation of
set equality — old or new — can catch *undeclared dependence*: an author
can simply not link at all, and nothing here objects. The equality only
ever caught the harmless half, *linked but undeclared*. Finding a claim
discussed without being cited is a search problem — keyword and semantic
search over the register, precision from the claim's own subject terms,
recall from semantic search over its text — not a link-check problem, and
C5 should stop pretending to be one.

> **Syntactic, not resolution — and the reason is not isolability.** An
> earlier draft said *"targets that resolve."* Under that reading a
> dangling `depends`/`because` entry with a matching prose link fails
> **both** C4 (or `orphaned-because`) and C5, because the unresolvable
> link would be excluded from `L`. Two errors for one defect is the
> lesser problem. The real problem is that **C5's message would lie**:
> the declared entry and its prose link agree perfectly in that case —
> both point at nothing — and reporting a divergence misdiagnoses it.
> C4/`orphaned-because` own "this target does not exist"; C5 owns "is
> every declaration linked." Keeping them syntactically separate keeps
> both diagnoses true.

**`orphaned-because`, the severity split C4 cannot express.** A dangling
`because` target gets its own check rather than a relaxed C4, because the
two diagnoses differ in kind: C4's "this claim is broken" would be a lie
here — the claim still holds, only its stated reason is gone.
`orphaned-because` runs at a strictly lower severity than every
C-numbered check: it is *reported*, and its presence alone never moves a
run from exit 0 to exit 1 (§5) — the same "reported, never failed on"
treatment README.md already gives an evaluator that discharges no claim.
Sharing C4's severity was considered and rejected: it would either block
a merge on a merely-thin justification, or, softened to match, silently
swallow real breakage — the severity split is the entire reason §1.2
distinguishes the two kinds at all.

**A bold-form definition's prose scope (§1.1) is narrower than a
heading's.** `L`'s window is "from the definition to the next heading
of the same or higher level" for heading form (unchanged), but "to the
next definition of *either* form, or the next heading of *any* level"
for bold form — a bold-form definition is a sentence inside a section,
not a section of its own, so nothing beneath even a subheading belongs
to it the way it would for a heading-form claim.

**`unregistered-definition`, derived the same way `normative-prose`
is, not a sixth numbered check.** §1.1's two forms are both recognized
independently of whether a claim block follows; a recognized definition
with no block is real corpus content this register does not yet cover.
`Warn` severity, like `orphaned-because` — never failed on, since a
corpus is expected to carry many of these on the day a registration
effort begins.

**`malformed-id`, likewise derived, not a numbered check.** §1.1's
recognizers require an id's inner text to be lowercase kebab-case; a
bracketed token that satisfies every other structural property of a
definition (line-start, the wrapper, immediately-following punctuation
for bold form; the whole heading text for heading form) but fails the
grammar is not a recognized definition, and so was previously silent —
not a claim, not `unregistered-definition`, not anything. `Warn`
severity, same as `unregistered-definition`: the remedy is a rename an
author makes, not a defect that should block a commit already in
flight. The id grammar itself is unchanged and is never relaxed to
accept what this check flags — accepting it silently would make
uniqueness (C2) case-insensitive in effect, which is exactly the
collision class C2 exists to catch.

**`unreachable-reference`, derived from `git`, not a sixth numbered
check.** A reference whose target *exists* is not automatically a
reference a reader can *reach*: a link into a gitignored working
directory (`.ledger`, `.scratch`, or any project-specific ignore rule)
resolves for its author, for a reviewer in the same worktree, and for
every check that runs where the author sits — and resolves to nothing
for every other reader of the repository, which is everyone the document
was written for
(`.ledger/2026-08-05-references-that-leave-the-register.md`, O4's
"unreachable" condition, distinct from C4's "dangles": the target is
present, only unreachable). `Fail` severity, not `Warn`: unlike
`unregistered-definition`/`malformed-id`, a real corpus is not expected
to carry any of these on the day this check ships — an unreachable
reference is wrong the moment it is written, with the same cheap,
unambiguous remedy C4's dangling `depends` has (rewire or remove).

**Directional.** Only a tracked document's link to an ignored path is
checked; the reverse — an ignored file linking into the repository — is
not, since only the repository's own reader-visible content is this
tool's concern at all. Scope is **path-shaped** references only, the
same distinction §1.3's prose-link normalization already draws between a
path and a bare claim-id citation: a link carrying a `/`, a `.`, or a
`#anchor` is checked; a bare kebab-case word with none of those reads as
a claim-id reference and is left alone (a claim id can never contain a
`.`, so the split is exact). Ignored-ness is asked of `git`
(`check-ignore`, batched once per corpus); a corpus that is not a git
repository, or an environment with no `git` on `PATH`, degrades to
silence rather than a false verdict or a crash — there is no
reader-reachability question to answer without a repository to ask.

**`normative-prose`, derived from `kinds = []`, not a sixth numbered
check.** §2's `kinds = []` already means a genre may hold no claim
blocks — which already means nothing normatively binding lives in it.
An RFC-2119 keyword (`MUST`, `MUST NOT`, `SHOULD`, `SHOULD NOT`, `SHALL`,
`SHALL NOT`, `REQUIRED`, `RECOMMENDED`, `MAY`, `OPTIONAL` — all-caps and
whole-word, since capitalisation is the only signal that distinguishes
the keyword from the ordinary English word) appearing in such a genre's
own voice is a binding assertion that happens to carry no block, which
is exactly how it evades C3: C3 only ever judges a claim that exists.
No new config field — the rule derives from `kinds` the same way
`explanation-forbids-kinds` derives from `quadrant`, and a genre that
permits at least one kind is unaffected, since its job is to say `MUST`.

Scope is the document's **own voice**. A decision record must be able to
quote a normative keyword in order to discuss or refute it, so a keyword
is exempt inside a block quote (any nesting depth), an inline code span,
or a fenced code block — including a claim block's own YAML, exempted
the same way any other code sample is, not as a special case. A heading
is own voice like any other text; nothing exempts it. This is why the
check belongs here rather than in a text-matching tool outside the
corpus: a regular expression over raw markdown cannot tell a quoted
`MUST` from an asserted one, but a parsed document tree already marks a
block quote and a code span as distinct nodes.

#### [normative-prose-own-voice]

A genre whose `kinds` is empty permits no RFC-2119 keyword in a scanned
document's own-voice text — text outside a block quote at any depth, an
inline code span, and a fenced code block. A genre that permits at least
one kind is unaffected.

```claim
kind: constraint
evaluator: test
```

No stem-uniqueness precondition exists: document identifiers are
corpus-relative paths (§1.3) and are therefore unique by construction.

## 4. Outputs

### 4.1 The index

Emitted to stdout as JSON, or to a path with `--out`. **Never committed** —
it is a pure projection of the source.

```json
{
  "claims": {
    "lock-groundness": {
      "file": "docs/specs/lock-file-schema.md",
      "line": 88,
      "kind": "constraint",
      "evaluator": "property-test",
      "depends": ["docs/models/composition-model#6", "docs/models/execution-model#2.4"],
      "because": []
    }
  },
  "documents": {
    "docs/specs/lock-file-schema": { "file": "docs/specs/lock-file-schema.md", "genre": "docs/specs/**" }
  }
}
```

**The `documents` map is keyed by the document path identifier of §1.3**, not
by basename. This example previously showed a bare basename and was caught by
the implementation rather than by review: keying the index by basename would
have reintroduced, at the index layer, exactly the collision that §1.3
eliminates at the resolution layer — and it would have done so silently, since
a collision there overwrites rather than errors.

Worth stating why it survived a draft. §1.3 changed identifiers from basenames
to paths and gave the reasoning, but the worked example three sections later
was not re-derived from it, so the document held its own rule and a violation
of that rule simultaneously. Nothing in docket catches this, because the
worked example above is a `json` fence, not a `claim` block — outside
anything the checks inspect, however many claim blocks the rest of this
document carries.

#### [index-shape]

The index's `claims` map is keyed by claim id; its `documents` map is
keyed by the document path identifier of
[reference-syntax](reference-syntax), never by basename.

```claim
kind: constraint
evaluator: test
depends: [reference-syntax]
```

### 4.2 Blast radius

```
docket blast <ref>
```

`<ref>` is a `depends`/`because` entry per §1.3: a claim id or a
`<doc-path>#<anchor>` document anchor. Prints, transitively, every claim
(and the document it lives in) that cites the given ref — the set a
reviewer must re-check if the cited claim or document section changes.
Cycles are reported, not followed twice.

**Both reference forms feed one graph, and so do both kinds.** A
document-anchor entry creates a reverse edge exactly as a claim-id entry
does — there is no second, lesser notion of citation for the anchor form.
A `depends` edge and a `because` edge are walked identically: §1.2 splits
the two by *severity on deletion* (C4 vs `orphaned-because`), not by
whether a reviewer must re-check the citer — a claim justified by a
changed target needs a new reason exactly as urgently as a reviewer needs
to know a dependent claim might now be false, so both belong in the same
blast radius. **Reporting *which* kind each edge in the result carries is
deliberately out of this MVP** — it is a pure computation over data
already in the index (which of `depends`/`because` an edge came from), so
it is fully derivable later without touching the schema; only the schema
was urgent (§1.2). A document anchor is never itself a *citer* (only a claim
carries `depends`/`because`), so once a claim citing an anchor is found,
the walk continues from that claim's own id exactly as it would from any
other claim-id node: an anchor is a valid starting point, never a graph
dead end partway through.

**A `<ref>` that does not resolve in the corpus is a usage error** (exit
2, §5) — the same treatment already given an unknown claim id, extended
to an unmatched document anchor. **A `<ref>` that resolves but has no
citers is not an error**: `blast` prints nothing and exits 0, since an
empty blast radius is a legitimate answer, distinct from "this ref names
nothing in the corpus."

#### [blast-semantics]

`docket blast <ref>` accepts either [reference-syntax](reference-syntax)
form as its argument. A document-anchor entry creates a reverse edge the
same way a claim-id entry does, and the transitive walk continues from a
citing claim's own id afterward — a document anchor is a valid starting
point but never itself further citable, since only a claim carries
`depends`/`because`. A `depends` edge and a `because` edge are walked
identically, undistinguished in this MVP's output. An argument that does
not resolve in the corpus is a usage error (exit 2); one that resolves
but has no citers prints nothing (exit 0).

```claim
kind: constraint
evaluator: test
depends: [reference-syntax]
```

### 4.3 Run

```
docket run <claim-id>
```

Executes `<claim-id>`'s declared evaluator and reports one of six
outcomes:

| outcome | means |
|:---|:---|
| `pass` | every marker naming this claim exited zero, and none of them checked nothing |
| `fail` | a marker naming this claim exited non-zero |
| `absent` | the claim declares an evaluator other than `none`/`review`, but no marker names this claim id anywhere in the corpus |
| `none` | the claim declares `evaluator: none` — an honest, unimplemented state; no marker is even looked for |
| `review` | the claim declares `evaluator: review` — a human or agent read the claim against its target and it holds; no marker is even looked for |
| `vacuous` | every marker naming this claim exited zero, but at least one of them is recognized as having checked nothing |

`review` and `none` are both never-runs-a-marker outcomes, but they are
not the same claim: `none` says nothing has been attempted; `review`
says a claim was checked, by testimony rather than by a command this
tool can re-execute. Collapsing the two would erase the one distinction
this evaluator exists to add — see README.md, "Why review is
irreducible."

`absent` and `fail` are kept apart rather than folded into one
"not discharged" result: they send the reader in opposite directions —
`absent` means write the marker, `fail` means fix what the marker
checks. `vacuous` is kept apart from both for the same reason: a command
that exits zero having checked nothing is neither a missing marker nor a
real failure — it means fix the marker's target (most commonly, a
renamed or deleted test), a third direction rather than a flavor of
either of the other two.

A claim can have more than one marker naming it. All must exit zero for
the claim to pass; if any exits non-zero the outcome is `fail`; if none
fails but at least one is recognized as vacuous, the outcome is
`vacuous`.

**Evaluator markers.** A marker is a line, anywhere in the corpus tree,
containing the literal text `docket:` followed by a claim id and a
command:

```
// docket: lock-groundness :: cargo test ground_values_only -- --exact
\* docket: spine-chain-complete :: tlc -config Model.cfg Model.tla
-- docket: no-double-spend :: alloy exec -c Model.als NoDoubleSpend
```

**The scanner never parses the comment leader.** `//`, `\*`, `--`, or
anything else preceding `docket:` is not recognized syntax — only the
literal token `docket:` is matched, wherever it appears on a line. This
is deliberate and language-agnostic: a corpus's evaluators are not all
one language (proofs in Lean, model checks in TLA+ and Alloy, tests in
whatever the implementation uses), and a comment lexer would have to be
written per language. Matching the token alone works identically across
all of them, at the cost of never inferring a marker's language from its
leader — the marker's own command is what runs, and it is `sh -c`'d in
the corpus root, so it can be written the way its author would type it
at a prompt.

**The vacuity opt-out.** A marker's id may carry a trailing `!`,
immediately after the id and before any whitespace —
`docket: <id>! :: <command>` — exempting that marker from vacuity
detection: its exit status alone is trusted, unconditionally. This
exists for an evaluator kind the runner has no output recognizer for, so
that evaluator can still report a genuine `pass`. It is a deliberate,
once-written assertion an author makes explicitly, never a default any
marker gets silently.

**Vacuity detection today recognizes `cargo test`'s output shape**: a
`test result: …` summary line reading `0 passed; 0 failed` — the exact
shape a typo'd, renamed, deleted, or `#[ignore]`d test name prints even
though the command still exits zero. Detection only ever *downgrades* a
recognized success; a command whose output matches no known shape is
`pass`, not `vacuous`.

#### [run-outcomes]

`docket run <claim-id>` executes every marker naming that claim id and
reports `pass` (every matching command exited zero and none checked
nothing), `fail` (any matching command exited non-zero), `absent` (the
claim names a real evaluator but no marker exists for it anywhere in the
corpus), `none` (the claim declares `evaluator: none`), `review` (the
claim declares `evaluator: review`), or `vacuous` (every matching
command exited zero but at least one is recognized as having checked
nothing). A marker's id may carry a trailing `!` to opt that marker out
of vacuity detection.

```claim
kind: constraint
evaluator: test
```

## 5. Exit codes

`check`'s exit code tracks its report's severity: a report holding only
`Warn`-severity diagnostics (`orphaned-because`, `unregistered-definition`)
still exits 0.

| command | code | meaning |
|:---|:--:|:---|
| `check` | 0 | all checks pass (`Warn`-only reports included) |
| `check` | 1 | one or more `Fail`-severity checks failed |
| `check`, `blast`, `run` | 2 | usage or configuration error (bad `docket.ncl`, unreadable path, unknown claim id, a `blast` ref or `run` claim id that doesn't resolve) |
| `run` | 0 | outcome `pass`, `none`, or `review` |
| `run` | 1 | outcome `fail` |
| `run` | 3 | outcome `absent` |
| `run` | 4 | outcome `vacuous` |

A caller gating CI on `run`'s exit code can tell "write the marker" from
"the evaluator regressed" from "the evaluator ran but checked nothing"
without parsing stdout — the same reasoning §3's severity split gives
`orphaned-because` against `C4` doesn't carry over to `run` unchanged:
`run` reports on exactly one claim, and its outcome already *is* the
whole severity; there is nothing left to aggregate the way `check`
aggregates many diagnostics into one process exit.

Suitable as a CI gate with no wrapper.

## 6. Deliberately out of scope

The verdict register, the stability metric, generated reference output,
and signing. Each needs the index (and, for the register, the runner) to
exist first, and none of them changes the shape above.

**The evaluator runner itself has shipped** — §4.3's `docket run` — and
is no longer out of scope. What remains out of scope is the
*whole-corpus* accounting built on top of it: a table over every claim's
`run` outcome, aggregated per kind into the conformance fractions
README.md's "Tying claims to the evaluators that discharge them"
describes. `run` answers "is this one claim discharged"; the register
would answer "is the corpus."

## 7. Implementation notes

- **Language:** the repository's choice; no constraint from this spec. The
  checks are pure functions over parsed input, so the natural shape is a
  library plus a thin CLI.
- **Nickel is required** for the contract and `docket.ncl` — today, that
  is its whole scope. `docket check` validates a claim block's YAML and
  a repository's `docket.ncl` by invoking `nickel export` against
  `contracts/*.ncl` (`docket::contract`) rather than reimplementing
  Nickel's own checking; the five checks themselves (§3) are plain Rust
  functions over the parsed corpus, not Nickel contracts. The longer-term
  intent is for the evaluator layer itself — the pure computations over
  the reference graph (reachability, orphan completeness, what becomes
  unreachable on a deletion) — to move into Nickel, with Rust reduced to
  parsing, filesystem walking, and the CLI surface. That migration has
  not happened; this spec describes what is built, not that intent, and
  "Nickel is required" should not be read as "Nickel evaluates claims."
- **The extractor must not use regular expressions over prose.** Parse
  markdown to a document tree, then walk it. Fenced blocks and heading
  levels are structure, and regex over structure is the brittleness this
  tool is meant to displace.
- **Test corpus:** a fixture directory of small markdown files, one per
  check, each a minimal instance that must fail exactly that check — plus
  a golden corpus where everything passes. Checks are the product, so the
  fixtures are the specification of correctness.
