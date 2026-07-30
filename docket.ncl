# docket's own genre configuration (MVP.md §2), dogfooding the tool
# against its own specification.
#
# This corpus is flat: MVP.md and README.md sit at the repository root,
# not under a docs/**-style tree, so each genre pattern below names a
# single file rather than a recursive glob.
{
  genres = [
    # MVP.md is the specification genre (README.md's genre table): it
    # states what must be true, so its claim kind is `constraint`.
    { path = "MVP.md", kinds = ["constraint"] },

    # README.md is explanation and holds no claims — explicit `kinds = []`
    # rather than leaving it unmatched, so a future claim block added here
    # is a loud C3 genre violation instead of silently unscanned. That is
    # the same choice this corpus's own design principle makes: "anything
    # an agent could get wrong must be checked by a gate."
    { path = "README.md", kinds = [] },
  ],
}
