# docket's own genre configuration (MVP.md §2), dogfooding the tool
# against its own specification.
#
# This corpus is flat: MVP.md and README.md sit at the repository root,
# not under a docs/**-style tree, so each genre pattern below names a
# single file rather than a recursive glob.
{
  genres = [
    # MVP.md is the specification genre (README.md's genre table): it
    # states what must be true, so its claim kind is `constraint`, and it
    # answers a "what is accurately true" question — Divio's reference.
    { path = "MVP.md", kinds = ["constraint"], quadrant = "reference" },

    # README.md is explanation and holds no claims — explicit `kinds = []`
    # rather than leaving it unmatched, so a future claim block added here
    # is a loud C3 genre violation instead of silently unscanned. That is
    # the same choice this corpus's own design principle makes: "anything
    # an agent could get wrong must be checked by a gate." Its
    # `quadrant = "explanation"` is not just a label here — under the
    # derived rule (MVP.md §2), it is *why* `kinds = []` is required
    # rather than merely conventional.
    { path = "README.md", kinds = [], quadrant = "explanation" },

    # `contracts/*.ncl` are Nickel, not markdown — no claim block can
    # live here, so `kinds = []` the same way README.md's does — but
    # they are this project's normative SPECIFICATION layer (MVP.md §7:
    # "Nickel is required for the contract"), continuous with MVP.md
    # rather than mere implementation. Declaring them as a genre is what
    # brings them into `unreachable-reference`'s widened surface
    # (MVP.md's "Which files this check covers"): a backtick-delimited
    # path citation here — `` `.ledger/…md` `` — is exactly as
    # unreachable to a stranger as one in ordinary prose.
    { path = "contracts/*.ncl", kinds = [], quadrant = "reference" },

    # fixtures/MANIFEST.md is explanation about the fixtures, not a spec
    # of its own — `kinds = []`, same reasoning as README.md's. Brings
    # its own `.ledger/…` citations (a documentation file like any
    # other) into `unreachable-reference`'s surface; its many inline
    # code spans illustrating link syntax (`` `[…](target-claim)` `` and
    # the like) are safe here — none resolves to a real gitignored path,
    # so they stay silent the same way an ordinary non-path code span
    # already does.
    { path = "fixtures/MANIFEST.md", kinds = [], quadrant = "explanation" },
  ],
}
