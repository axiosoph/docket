{
  genres = [
    { path = "docs/specs/**",        kinds = ["constraint"], quadrant = "reference" },
    { path = "docs/models/**",       kinds = ["invariant"], quadrant = "reference" },
    { path = "docs/architecture/**", kinds = ["requirement"], quadrant = "reference" },
    { path = "docs/adr/**",          kinds = [], quadrant = "explanation" },
  ],
}
