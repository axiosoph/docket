# docket

**A register of claims across a documentation corpus.** Requirements,
invariants, and constraints get an identifier, exactly one home, and a
named evaluator — so that *"is this project stable?"* becomes a computed
number with an enumerated residue, instead of a feeling.

> Status: **implemented.** `check` runs the five structural checks below
> plus three further diagnostics (`orphan-claim`, `normative-prose`,
> `unregistered-definition`); `blast` computes the citation graph and its
> transitive closure; `run` executes a claim's evaluator and reports one
> of six outcomes (see "Tying claims to the evaluators that discharge
> them," below). The coverage index, the verdict register, and the
> stability metric described later in this document are not built —
> "Out of MVP scope" marks what's still missing. Name is provisional.

---

## The problem

A specification-first project accumulates several genres of document —
decision records, formal models, normative specifications, architecture
overviews, user documentation. Each answers a different question. In
principle they compose; in practice they **drift**, and the drift is
structural rather than careless:

- **Duplication is the drift surface.** When the same fact is stated in
  four documents, there are four places to update and four places to
  diverge. Divergence is then discovered by a reader, months later, and
  usually by being misled.
- **Status is asserted, so it rots.** A constraint annotated
  `VERIFIED: unverified` looks identical to a discharged one at a glance,
  and nothing forces the annotation to match reality.
- **Conventions decay under machine authorship.** Documentation genres are
  usually kept apart by habit. Habits are not something a language model
  follows reliably, and generation is *additive by construction* — an LLM
  corrects by appending, rarely by cutting. So an agent-maintained corpus
  accretes correction layers and grows monotonically, appearing diligent
  while getting worse.

The last point is why this is a tool rather than a style guide. Under
heavy machine modification, anything that depends on discipline will fail.

## Two principles

Everything here follows from these:

> **1. Anything an agent could get wrong must be checked by a gate, not
> documented as a convention.**
>
> **2. Anything an agent could assert must be generated, not written.**

Applied: genre boundaries become a gate rather than an editorial norm;
claim status becomes a build artifact rather than an annotation; reference
documentation becomes a projection of source rather than prose to
maintain.

## Genres, and one home per fact

Each genre answers exactly one question, and may hold exactly one kind of
claim. **Genres may cite; they may never restate.**

| genre | answers | claim kind |
|:---|:---|:---|
| decision record | *why* | none — terminal justification, immutable |
| formal model | *what law holds* | `invariant` |
| specification | *what must be true* | `constraint` |
| architecture overview | *how it fits together* | `requirement` |
| user documentation | *how it is used* | (evaluator: executable example) |
| reference | — | **generated**, never written |

Two consequences worth stating plainly:

**A decision record that carries normative content will need revising**,
which its genre forbids — a decision is superseded, not amended. Revision
layers inside a decision record are the symptom of a genre violation, and
they are how the most-drifted document in a corpus usually got that way.

**An architecture overview's drift risk is proportional to the substance
it holds.** One that carries diagrams, boundaries, and pointers cannot
contradict anything. One that restates invariants will, and its readers
will trust the stale copy.

### Why exactly three kinds

*This is an argued correspondence, not a proved one — a working
hypothesis, and a genuinely fourth-kind claim would be evidence against
it rather than a bug to route around.*

Three kinds is not an arbitrary taxonomy. Each corresponds to a different
question a corpus's own artifacts must answer about a claim:

| kind | genre | bounds |
|:---|:---|:---|
| `requirement` | architecture overview | **determination** — is this settled by the system's own record at all? |
| `invariant` | formal model | **certifiability** — is it checkable, and at what power? |
| `constraint` | specification | **monotonicity** — what is true and must *remain* true as the system grows |

The strongest evidence for the correspondence is that each kind's
natural evaluator is exactly what its axis demands — chosen for
independent reasons, before anyone noticed the pattern. Determination is
a scoping question: a requirement carries no proof and no test, only a
check that it is *about* something the system actually produces.
Certifiability is proof or model-check — the exact machinery formal
models carry and nothing else in a corpus does. Monotonicity is *a test
that must keep passing as the system grows*, and a regression suite is
precisely that: "this was true and has stayed true across every commit
since." A property test is the same claim quantified harder.

That specifications get tests and models get proofs is normally treated
as an engineering convention. Under this reading it is forced by which
axis each genre bounds — which also predicts something about the grade
hierarchy below: a claim quantified over all inputs (monotonicity,
pushed to its limit) will never reach type-grade discharge, however the
API is designed, because a type can only rule out what it can represent,
not what must remain true across an unbounded future.

The prediction that keeps this honest: **a claim that genuinely fits
none of the three kinds refutes the correspondence.** User-documentation
claims (discharged by an executable example) are the obvious test —
if their natural axis turns out to be monotonicity ("an example that
must keep running"), they are constraints in a different genre, not a
fourth kind. Decision records, which hold no claims at all, are
consistent for the same reason: a decision is not a claim about the
system, so it bounds nothing.

## The claim block

Prose stays prose. Machine-tractable metadata rides alongside it in a
fenced block, validated by a committed Nickel contract:

````markdown
### [lock-groundness]

Every lock value MUST be ground: names bound to content identities and
exact version strings.

```claim
kind: constraint
evaluator: property-test
depends: [composition-model#6, execution-model#2.4]
```
````

Fenced blocks rather than inline XML or HTML attributes, for one reason:
**a language model writes a fenced block with a schema correctly far more
often than it writes nested markup.** Unclosed tags and attribute drift
are precisely the error class this tool exists to eliminate.

The *query* surface is a separate concern. Any pure projection of the
source will do — index, HTML, graph — because a pure transformation is
equally safe to operate on either side.

## What the block holds, and what it must not

| in the document | in the generated register |
|:---|:---|
| `id`, `kind`, `evaluator` **name**, `depends`, `because` | the evaluator's **verdict** |
| stable across runs | recomputed every run |

**Verdicts are never written.** The register is produced by running the
evaluators, so *the absence of a passing evaluator is the unverified
state* — discovered rather than asserted. Nothing can drift from reality,
because nothing claims anything about reality.

The register is a pure projection and is therefore **not committed**.

## References are typed, and a third kind is load-bearing

A reference is not one undifferentiated `cites`. Deleting the target of a
reference means something different depending on *why* the reference was
made, so the block declares two kinds explicitly and leaves a third
undeclared:

| kind | meaning | on deletion of the target |
|:---|:---|:---|
| `depends` | the claim's truth or meaning requires the target | the claim is **broken** — rewire or remove |
| `because` | the claim's justification is the target | the claim **still stands**, under-justified — restate the reason, or discover it was vestigial |
| bare reference | context; asserts no dependence | nothing |

The closest analogue is a runtime versus a build dependency: a missing
runtime dependency means the artifact doesn't work; a missing build
dependency means it still works but can no longer be *derived*. That maps
the remedies exactly, which is the test of an analogy — not that the
words fit, but that the consequences do.

**The third kind is not optional.** Without it, an author has only one
field to reach for, and reaches for it every time a citation is merely
informative. Every deletion then manufactures false breakage — an
incidental mention treated as load-bearing — or, worse, teaches authors
to stop declaring references at all to avoid the noise. A register
nobody trusts is worse than none, because its silence reads as safety.
Bareness gets no field of its own: it is the *absence* of a declaration,
and a field that let an author assert it would be a declaration of
absence, which is incoherent.

**These declarations are not verifiable.** Nobody can prove a claim
really depends on a target rather than merely mentioning it — a `depends`
written where a `because` belongs is a review finding, not something a
gate can catch. The register exists to **bound the review surface, not
eliminate it**, which is the argument for keeping the vocabulary this
small (two declared kinds, not a weighted or graded dependence) rather
than trying to be clever about intent.

A markdown link in prose is presentation; `depends` and `because` are the
graph edges a lint checks it against, so declaration and prose cannot
diverge. A normative change's **blast radius** is computed over the
declared edges, never by pattern-matching prose — and both kinds enter
the same graph, walked identically: a reviewer must re-check a dependent
claim and a justified-by claim alike, since a claim depending on a
changed target may now be wrong and a claim justified by it may now need
a new reason. The kinds differ in what deletion means, not in whether a
downstream change matters. A reference names either a claim or a
document section directly, so the walk runs the same way from either
end.

## "Stable", defined honestly

> **Stable = every documented claim has a passing evaluator, and the
> residue is enumerated rather than unknown.**

Half of that is measurable and half is not, and conflating them
overstates the guarantee:

- **Conformance is measurable.** It is a fraction over the register, with
  a named residue. It moves monotonically as work lands.
- **Completeness against *reality* is not.** You cannot count claims
  nobody wrote about behaviour nobody built. Exhaustiveness against reality
  is undecidable, so that half stays a judgment.
- **But completeness against the *code* is measurable**, and that is the
  half that matters for *"the code conforms."* See below.

### Exhaustiveness is undecidable in general and bounded by the corpus in practice

The undecidable question — *did we document everything?* — has a decidable
neighbour: **what code is unaccounted for?** Public surface that no claim
covers is a finite, enumerable set, and every member carries an action:

| the unclaimed surface is | the action |
|:---|:---|
| load-bearing and undocumented | **write the claim** |
| vestigial | **delete the code** |
| legitimately internal | **mark it so** — and that mark is a vouch (below) |

Either of the first two is progress. Which is the point: **an unclaimed
public item is never merely untidy, it is always a signal with a
disposition.**

Note this is the mirror of the evaluator check above — *an evaluator
discharging no claim* — generalized from evaluators to surface. Both
directions of the same relation, and neither is instrumented by
conventional tooling: coverage measures whether code ran, never whether
code is *accounted for*.

**Granularity is public surface, not every function.** Private helpers are
implementation and requiring claims of them would be noise that erodes the
signal. A *public* item is a contract, so a public item covered by no claim
is either an undocumented contract or an over-exposed internal — and both
of those are worth knowing.

**The third case needs a suppression marker or the signal decays.** Some
items are public for mechanical reasons and are genuinely not contracts.
Marking one as internal is an explicit human assertion that it needs no
claim — which is to say **it is a vouch**, attributable like any other, and
counted as one rather than vanishing from the books. Consistent with the
rest of this design: the residue is never hidden, only named.

So the honest statement of the pair:

> **Exhaustiveness against reality is undecidable. Exhaustiveness against
> the code is not — it is the public surface no claim covers, and that set
> is finite, enumerable, and actionable.**

It is a bound rather than an equivalence: claims can exist about things no
code implements yet, so unclaimed surface under-approximates the whole
documentation gap. What it bounds exactly is the gap *over the implemented
surface*, which is precisely the half *"the code conforms"* is about.

This matters because *"the documentation is complete and the code
conforms"* is the intuition worth chasing, and only its second half can
ever be a number.

## Tying claims to the evaluators that discharge them

Line and branch coverage answer *"was this function exercised?"* They say
nothing about whether a **declared claim** is checked, and the two are
orthogonal: a corpus can have complete line coverage and zero claims
verified. So the register needs the link from claim to the specific
evaluators that discharge it — not merely "this claim names an evaluator
kind."

Corpora already do this by hand, in prose, unverifiably — annotations that
name specific test functions beside a constraint, which then drift when
tests are renamed. That is the practice to mechanize.

**The link is declared at the evaluator, and coverage is generated.**
Per principle 2: coverage is a fact about the corpus, so it must be
discovered rather than asserted. A claim block keeps declaring which *kind*
of evaluator it expects; which evaluators actually discharge it is a
finding.

It is declared in the evaluator rather than the document for a second
reason: **tests churn far more than claims**, so the reference belongs in
the artifact that moves. A renamed test cannot silently un-discharge a
claim, because its marker travels with it.

### The marker spans languages, because evaluators are heterogeneous

```rust
// docket: lock-groundness
#[test]
fn ground_values_only() { … }
```

A comment marker, not a language attribute. The reason is not
convenience: a corpus's evaluators are **not all one language**. Proofs
live in Lean, model checks in TLA+ and Alloy, property and unit tests in
the implementation language, examples in CLI transcripts. A Rust attribute
cannot mark a Lean theorem or a TLA+ module; a line comment can mark all of
them.

*This does not contradict "no regular expressions over prose."* A marker is
**structure** — an exact token in a known position. The prohibition is on
inferring structure from prose, which is a different thing.

### The check nobody instruments: evaluators that discharge nothing

The reverse direction is as informative as the forward one. **An evaluator
declaring no claim is suspect** — either it exercises something the corpus
never declared (a missing claim), or it tests an implementation detail
(legitimate, but it should not count toward the metric).

Reported, never failed on. But the ratio matters: a project where most
evaluators discharge no claim has a verification surface disconnected from
its documentation surface, and no aggregate number would reveal it.

### The metric is per kind, and that is the whole point

A single fraction hides precisely the gap worth finding. A corpus can have
every *constraint* discharged and not one *requirement* demonstrated, and
an aggregate would look healthy.

So conformance is reported per kind:

```
requirement   2/3    ( 67%)
invariant     5/7    ( 71%)
constraint   14/14   (100%)
```

**Which is what distinguishes this from code coverage.** Coverage measures
the low-level surface; this measures whether the *high-level declarations*
are checked — and per-kind is the only shape in which that question has an
answer.

If the correspondence between kinds and their three axes holds ("Why
exactly three kinds," above), then per-kind conformance reads as **which
axis is under-closed**, which makes that correspondence actionable
rather than decorative.

### The same marker reaches load-bearing API surface — at a higher grade

An API can discharge a claim, and it does so **differently in kind** from a
test:

| | what it establishes |
|:---|:---|
| a test | *this case passed* — evidence of conformance |
| **a type** | *the violation does not exist* — enforcement by construction |

A phantom-typed digest disjoint from a backend object id does not *test*
that the two are never confused; it makes the confusion **unrepresentable**.
A set type exposing only a join does not test commutativity; it removes the
means to violate it. A verdict with no boolean conversion does not test that
its residue is non-erasable; erasure fails to compile.

So the register records a **grade**, not a boolean, along the standard
hierarchy for evidence that is re-runnable:

```
proof  >  type  >  property test  >  example test  >  linter
```

`review` is not this chain's weakest rung, and does not appear in it at
all — it is a different evidence species entirely ("Why review is
irreducible," below), so ranking it below `linter` would misstate what
it is. The claim block's own `evaluator` field (MVP.md §1.2) is exactly
this hierarchy plus `review` named as its own value, not its bottom one,
and `none` for "not yet discharged."

Mechanically this is the same marker in the same place, which is the point
— one mechanism, two capabilities:

```rust
// docket: czd-oid-disjoint
pub struct Czd<T>(…);
```

**Two things this buys that a boolean cannot.** It distinguishes *how
strongly* each claim is held. And it surfaces a refactoring signal no tool
gives today: **a claim discharged at a lower grade than it could be** — a
constraint guarded by a test where a type could make the violation
unrepresentable. That is a queryable list rather than an insight someone
has to happen to have.

### The hierarchy is normative, not merely descriptive

It is a **preference order**, not a scale for reporting. Where a claim can
be discharged elegantly at a higher grade, it should be — a violation that
cannot be written beats one caught when written.

So a grade gap is not neutral information. The mechanical part is the
signal (*discharged at N; N+1 appears reachable*); the verdict stays human,
because a type-level solution reached by contortion is worse than the test
it replaced. Surfaced by the machine, judged by a person — the same split as
everywhere else here.

**The gradient also predicts which obligations are irreducible**, which is
its most useful property. Types enforce *structural* claims — this cannot be
represented, this cannot be called without that. Proofs establish *semantic*
ones — this holds over all inputs, or all reachable states. So a claim
quantified over all inputs will never reach type-grade, and no amount of
API design will move it. **Counting those tells a project exactly how much
proof it actually needs**, rather than deciding by architectural instinct up
front.

### Where type-discharge's honesty limit sits, stated rather than hidden

The machine confirms the marked item exists and compiles. **It cannot
confirm the type genuinely enforces what the claim says** — a type could be
marked and enforce nothing.

So type-discharge splits: the machine half is *"it exists and the build
passes"*; the faithfulness half is **review-established**. That is exactly
the epistemics of a machine-checked proof, where the checker verifies the
proof and a human verifies the *definition* is faithful. Same honesty
discipline, applied one grade down; it must be reported as
review-established rather than silently counted as machine-established.

A partial machine check is possible per language — confirming the marked
item is a type rather than a function — but it is a per-language
enhancement, not a precondition.

### Why review is irreducible

*Argued, not proved — falsifiable, and untested against a real register.*

The grade hierarchy above — proof, type, property test, example test,
linter — plus `review` is not five mechanical checks plus a human
fallback for whatever nothing mechanical covers. It is two different
*species* of evidence, and naming the difference is what makes review a
first-class grade rather than an embarrassment to eliminate.

A **corroboration** is a re-verification of an artifact against its own
content — a check anyone else could re-run: a proof, a type, a test. A
**vouch** is a judgment binding the artifact to the person who made it —
testimony, which no one else can re-run. Every mechanical evaluator above
is a corroboration; review is this register's vouch, by definition
rather than resemblance.

That partition predicts something about a stable corpus: **a claim needs
both, not either-or.** A claim with a passing test that nobody has read
for *whether the test tests the right thing* is corroboration with no
vouch — the green-by-construction failure an adversarial test-surface
review exists to catch. A reviewed claim with no evaluator is a vouch
with no corroboration — testimony standing in for a check that was never
built. Neither closes a claim alone; a stable corpus wants both columns
per claim, not one.

This is also the argument for building a register over documentation at
all, rather than trusting vigilance. Exhaustiveness is the one property
generation cannot self-verify: an omission is invisible from inside, so
there is no gradient toward completeness, and a confident partial answer
is indistinguishable from a complete one to whoever wrote it. Review is
also the most expensive evaluator after proof, so the only way to have
it *at all* is to bound what needs it — which a deterministic enumerator
does and vigilance cannot. Locate the residue, name it, and stop
pretending it isn't there: that is the justification for computing a
register over documents in the first place, not a footnote to it.

### Why a deterministic index rather than careful reading

*"Which API surfaces enforce which invariants"* is an exhaustive
cross-cutting question over a large corpus, and it is precisely the shape
where a language model returns a confident partial answer. There is no
gradient toward exhaustiveness in generation, so the omissions are
invisible from inside.

The consequence is about review, not tidiness. A human auditing machine-authored
work needs to establish *"was this claim actually enforced?"* — and the cost
of that answer determines whether review is feasible at all. **A register
makes it a query; without one it is a code read.** Given that code review is
slow by definition, moving the audit from diff level to declaration level is
what makes the human side of the loop tractable.

### Out of MVP scope, but it disturbs nothing

This needs the evaluator runner as a prerequisite, and the runner has
since shipped: `docket run <claim-id>` executes a claim's marker and
reports `pass`/`fail`/`absent`/`none`/`review`/`vacuous` (see MVP.md's "Run"
section). What has **not** shipped is this section's own feature — a
whole-corpus coverage index reporting per-kind conformance, generated
from every claim's discharge — which is a table over every claim's `run`
result, not a single invocation of it. It requires **no change to the
claim block** — the block declares an expected evaluator kind; discharge
is discovered separately. The MVP's format is forward-compatible as
written.

## Generality

Nothing here is specific to one project, and genre boundaries differ
between corpora. So **genres are configuration**: a declared mapping from
path pattern to permitted claim kinds. A repository states its own
hierarchy, and the gate enforces whatever it stated.

The payoff of sharing the tool across projects is not code reuse. It is
**one definition of stable, comparable across corpora** — the same metric
measured in more than one instance.

## MVP scope

Deliberately smaller than the design above. The register and the metric
need something to measure; the writing pass needs only tractability.

**In:**

1. A Nickel contract for the claim block.
2. An extractor: pull fenced `claim` blocks from markdown.
3. An index: `id → (file, kind, evaluator, depends, because)`.
4. Five checks:
   - blocks validate against the contract
   - ids are unique corpus-wide
   - **`kind` is permitted by the genre its path declares**
   - every `depends` target resolves
   - every `depends`/`because` entry has a matching prose link
5. A blast-radius query: given a claim id or a document anchor, what
   depends on or is justified by it, transitively.

**Out, until there is something to measure:** the verdict register, the
metric, generated reference output, signing. (The evaluator runner
itself — `docket run` — has since shipped; see MVP.md's "Run" section.
The register and metric described above are a whole-corpus table over
many `run` invocations, and that table still doesn't exist.)

Check 3 carries most of the value. It makes the genre hierarchy
**mechanically enforced** rather than editorially intended — and with it,
duplication is *structurally prevented* rather than periodically measured,
because a genre that may only cite has nowhere to put a second copy.

## Non-goals

- Judging whether a document is *correct*. That stays human, and stays
  hard.
- Detecting semantically duplicated claims — the same fact under two ids.
  The genre gate removes most of the room for it; the remainder is a
  judgment call and should not be automated into a false negative.
- Replacing prose. Every claim is a sentence a person reads; the block
  beside it is for the machine.
