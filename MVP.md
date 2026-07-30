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
cites: [docs/models/composition-model#6, docs/models/execution-model#2.4]
```
````

**The claim's `id` is not in the block.** It is taken from the nearest
preceding heading whose text is exactly a bracketed kebab-case token:

```markdown
### [lock-groundness]
```

Rationale: the id already exists in prose as the human-readable anchor,
and duplicating it into the block would create precisely the divergence
surface this tool exists to remove. One statement, one place.

A claim block with no such preceding heading in the same file is an
error (`orphan-claim`).

### 1.2 Fields

| field | required | type |
|:---|:--|:---|
| `kind` | yes | one of `requirement`, `invariant`, `constraint` |
| `evaluator` | yes | one of `proof`, `model-check`, `type`, `property-test`, `test`, `example`, `none` |
| `cites` | no (default `[]`) | array of refs |

Unknown fields are an error, not ignored. Tolerating them would let a
typo'd field name silently carry no meaning.

### 1.3 Reference syntax

A `cites` entry is either:

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

**Ref syntax is enforced by the contract, not by C4.** A `cites` entry
whose *shape* is invalid — whitespace, a second `#`, an empty half — is a
malformed block and fails **C1**. C4 therefore only ever operates on
well-formed refs, and "does this resolve" presupposes "is this a ref."

**Prose-link normalization.** A markdown href in prose is normalized to a
ref before comparison (C5): **resolve it against the containing file's
directory** to get a corpus-relative path, strip the `.md` extension, retain
any `#fragment`, and treat a fragment-only href as referring to the
containing document. So `[…](../models/composition-model.md#6)`, written in
a file under `docs/specs/`, normalizes to `docs/models/composition-model#6`.

This is load-bearing rather than cosmetic: **real corpora link by relative
path with an extension**, and without normalization those links would never
match a `cites` entry, making C5 fire on every correctly-linked claim.

**A leading `/` is corpus-root-relative**, not joined onto the citing file's
directory. So `[…](/docs/models/composition-model.md#6)` normalizes to
`docs/models/composition-model#6` from any file in the corpus. This follows
rendered-markdown convention, where a leading slash is site-root-relative, and
the alternative is incoherent: joining `/docs/...` onto a containing directory
produces a path that names nothing.

Note the normalization *resolves* the path rather than discarding it, which
is what makes it agree with §1.3's path-based refs — and it means an
ordinary relative markdown link, written the way an author would write it
anyway, normalizes to exactly the ref a `cites` entry carries. A link that
escapes the corpus root is not ref-shaped and is ignored.

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

#### [reference-syntax]

A `cites` entry is a claim id or a `<doc-path>#<anchor>` document anchor;
document identifiers are the corpus-relative path with `.md` removed, not
a basename; a prose link normalizes to the same vocabulary by resolving
against the citing file's directory before comparison; an anchor matches a
heading by non-alphanumeric-bounded prefix, not exact text.

```claim
kind: constraint
evaluator: test
cites: []
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
cites: []
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
| `C4` | every `cites` target resolves | dangling reference |
| `C5` | prose links and `cites` agree, per claim | the graph and the prose have diverged |

**C5, precisely.** For a claim, let `L` be the set of markdown link
targets appearing in its prose body (from its id heading to the next
heading of the same or higher level, excluding the claim block itself),
restricted to targets that are **ref-shaped** per §1.3 — a syntactic
filter, *not* a resolution filter. Then `L` and `cites` must be equal as
sets. A link to an external URL is ignored, since it is not ref-shaped.
Rationale: `cites` is authoritative for the graph, but a reader follows
prose, so the two must not disagree.

> **Syntactic, not resolution — and the reason is not isolability.** An
> earlier draft said *"targets that resolve."* Under that reading a
> dangling `cites` entry with a matching prose link fails **both** C4 and
> C5, because the unresolvable link would be excluded from `L`. Two errors
> for one defect is the lesser problem. The real problem is that **C5's
> message would lie**: prose and `cites` agree perfectly in that case —
> both point at nothing — and reporting a prose/`cites` divergence
> misdiagnoses it. C4 owns "this target does not exist"; C5 owns "the two
> representations disagree." Keeping them syntactically separate keeps both
> diagnoses true.

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
cites: []
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
      "cites": ["docs/models/composition-model#6", "docs/models/execution-model#2.4"]
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
of that rule simultaneously. Nothing in docket catches this, because `MVP.md`
carries no claim blocks — which is a precise statement of the gap, and the
argument for closing it.

#### [index-shape]

The index's `claims` map is keyed by claim id; its `documents` map is
keyed by the document path identifier of
[reference-syntax](reference-syntax), never by basename.

```claim
kind: constraint
evaluator: test
cites: [reference-syntax]
```

### 4.2 Blast radius

```
docket blast <ref>
```

`<ref>` is a `cites` entry per §1.3: a claim id or a `<doc-path>#<anchor>`
document anchor. Prints, transitively, every claim (and the document it
lives in) that cites the given ref — the set a reviewer must re-check if
the cited claim or document section changes. Cycles are reported, not
followed twice.

**Both reference forms feed one graph.** A document-anchor `cites` entry
creates a reverse edge exactly as a claim-id entry does — there is no
second, lesser notion of citation for the anchor form. A document anchor
is never itself a *citer* (only a claim carries a `cites` list), so once
a claim citing an anchor is found, the walk continues from that claim's
own id exactly as it would from any other claim-id node: an anchor is a
valid starting point, never a graph dead end partway through.

**A `<ref>` that does not resolve in the corpus is a usage error** (exit
2, §5) — the same treatment already given an unknown claim id, extended
to an unmatched document anchor. **A `<ref>` that resolves but has no
citers is not an error**: `blast` prints nothing and exits 0, since an
empty blast radius is a legitimate answer, distinct from "this ref names
nothing in the corpus."

#### [blast-semantics]

`docket blast <ref>` accepts either [reference-syntax](reference-syntax)
form as its argument. A document-anchor `cites` entry creates a reverse
edge the same way a claim-id entry does, and the transitive walk
continues from a citing claim's own id afterward — a document anchor is
a valid starting point but never itself further citable, since only a
claim carries a `cites` list. An argument that does not resolve in the
corpus is a usage error (exit 2); one that resolves but has no citers
prints nothing (exit 0).

```claim
kind: constraint
evaluator: test
cites: [reference-syntax]
```

## 5. Exit codes

| code | meaning |
|:--|:---|
| 0 | all checks pass |
| 1 | one or more checks failed |
| 2 | usage or configuration error (bad `docket.ncl`, unreadable path) |

Suitable as a CI gate with no wrapper.

## 6. Deliberately out of scope

The evaluator runner, the verdict register, the stability metric,
generated reference output, and signing. Each needs the index to exist
first, and none of them changes the shape above.

## 7. Implementation notes

- **Language:** the repository's choice; no constraint from this spec. The
  checks are pure functions over parsed input, so the natural shape is a
  library plus a thin CLI.
- **Nickel is required** for the contract and `docket.ncl`. Validation
  should invoke Nickel rather than reimplementing its checking.
- **The extractor must not use regular expressions over prose.** Parse
  markdown to a document tree, then walk it. Fenced blocks and heading
  levels are structure, and regex over structure is the brittleness this
  tool is meant to displace.
- **Test corpus:** a fixture directory of small markdown files, one per
  check, each a minimal instance that must fail exactly that check — plus
  a golden corpus where everything passes. Checks are the product, so the
  fixtures are the specification of correctness.
