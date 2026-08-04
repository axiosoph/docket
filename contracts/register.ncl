# The register evaluator (.ledger/2026-07-30-reference-kinds-and-document-resolution.md,
# R6): given an already-extracted corpus as data, compute the index
# (MVP.md §4.1) and every check's diagnostics — C1-C5, orphan-claim,
# orphaned-because, normative-prose, unregistered-definition,
# malformed-id — in one pure evaluation.
#
# Rust does the extraction (extract.rs/corpus.rs: markdown to a document
# tree, filesystem walking) and src/checks.rs serializes the result into
# the shape this file expects; nothing downstream of "here is the set of
# claims with their typed references" happens in Rust. One `nickel
# export` invocation of `evaluate` replaces what was N invocations (one
# per claim, C1) plus a separate Rust-native pass for every other check.
#
# Usage: `evaluate <input>` where `<input>` is the record documented at
# `Input` below. Driven from Rust via a small expression piped over
# stdin (see `nickel.rs::evaluate_register`), the same mechanism the
# retired per-claim contract call used.
#
# claim.ncl's predicates are imported directly rather than re-declared —
# duplicating a validity rule between the single-claim contract and this
# batch validator would be exactly the drift this migration exists to
# remove.
let claim = import "claim.ncl" in

# ---------------------------------------------------------------------
# Ref grammar: <doc-path>#<anchor> or a bare claim id (MVP.md §1.3).
#
# This necessarily duplicates `model::CiteRef::parse` /
# `model::anchor_matches` on the Rust side, which stay there unchanged
# for `blast.rs` and `docket blast` (`main.rs`) — both explicitly out of
# this migration's scope, and both depend on the parsed `CiteRef` type
# directly. That boundary, not a choice made here, is why the same small
# grammar has two implementations in two languages rather than one.
#
# PROVISIONAL, pending a head decision: `anchor_matches`'s prefix rule
# and the bare-id prose-link form (`normalize_prose_link`'s
# no-anchor/no-slash branch, `prose_link_declares`'s bare-token match)
# are both invented syntax, conflict with the standard GitHub-slug
# anchors every other tool in this project resolves, and are a likely
# collapse to a single ordinary-markdown-link form. Kept isolated in
# this section on purpose — `anchor_matches` is the one function to
# replace with real slug computation; `normalize_prose_link`/
# `prose_link_declares` would each lose one branch — so that decision,
# if made, touches a bounded, named surface rather than something
# threaded through every check. Not collapsed here: the decision isn't
# made, and `model::anchor_matches` (Rust) would need the same change in
# lockstep for `docket blast` to keep resolving anchors consistently.
# ---------------------------------------------------------------------

# Split `s` on the FIRST occurrence of `sep` only (matches Rust's
# `str::split_once`, not a full `split`) — a second `sep` later in the
# string stays part of the tail.
let split_first = fun sep s =>
  let parts = std.string.split sep s in
  if std.array.length parts <= 1 then
    { head = s, tail = null }
  else
    {
      head = std.array.first parts,
      tail = std.string.join sep (std.array.drop_first parts),
    }
in

# A depends/because/normalized-prose-link target.
let parse_ref = fun raw =>
  let split = split_first "#" raw in
  if split.tail == null then
    { tag = 'claim, id = raw }
  else
    { tag = 'doc_anchor, path = split.head, anchor = split.tail }
in

let ref_to_string = fun r =>
  if r.tag == 'claim then r.id else "%{r.path}#%{r.anchor}"
in

let ref_eq = fun a b =>
  a.tag == b.tag
  && (
    if a.tag == 'claim then
      a.id == b.id
    else
      a.path == b.path && a.anchor == b.anchor
  )
in

# MVP.md §1.3 "Anchor derivation": anchor A matches heading text H iff H
# begins with A followed by end-of-string or a non-alphanumeric.
let anchor_matches = fun heading_text anchor =>
  let alen = std.string.length anchor in
  let hlen = std.string.length heading_text in
  if hlen < alen then
    false
  else if std.string.substring 0 alen heading_text != anchor then
    false
  else if hlen == alen then
    true
  else
    !std.string.is_match "^[a-zA-Z0-9]$" (std.string.substring alen (alen + 1) heading_text)
in

# Whether a (well-formed, per C1) ref resolves in the corpus.
let resolves = fun claim_ids documents cite =>
  if cite.tag == 'claim then
    std.array.elem cite.id claim_ids
  else
    let matching = std.array.filter (fun d => d.doc_path == cite.path) documents in
    std.array.length matching > 0
    && std.array.any (fun h => anchor_matches h.text cite.anchor) (std.array.first matching).headings
in

# ---------------------------------------------------------------------
# C1: claim block validation. A hand-rolled batch validator, not a
# contract application, because a contract applied to an array aborts
# at the first violating element (confirmed directly against Nickel
# 1.17.0) — this must report every malformed claim in one evaluation,
# the property the per-claim `nickel export` loop it replaces already
# had.
# ---------------------------------------------------------------------

let claim_field_violations = fun c =>
  if c.raw_error != null then
    [ "invalid YAML: %{c.raw_error}" ]
  else if !std.is_record c.raw_value then
    [ "claim block is not a YAML mapping" ]
  else
    let fields = std.record.fields c.raw_value in
    let unknown = std.array.filter (fun f => !std.array.elem f claim.KnownClaimFields) fields in
    let missing = std.array.filter (fun f => !std.array.elem f fields) claim.RequiredClaimFields in
    let has_kind = std.array.elem "kind" fields in
    let has_evaluator = std.array.elem "evaluator" fields in
    let has_depends = std.array.elem "depends" fields in
    let has_because = std.array.elem "because" fields in
    (if std.array.length unknown > 0 then
      [ "unknown field(s): %{std.string.join ", " unknown}" ]
    else
      [])
    @ (if std.array.length missing > 0 then
      [ "missing required field(s): %{std.string.join ", " missing}" ]
    else
      [])
    @ (if has_kind && !claim.KindPred c.raw_value.kind then
      [ "field `kind` has an invalid value" ]
    else
      [])
    @ (if has_evaluator && !claim.EvaluatorPred c.raw_value.evaluator then
      [ "field `evaluator` has an invalid value" ]
    else
      [])
    @ (if has_depends
      && !(
        std.is_array c.raw_value.depends
        && std.array.all claim.RefPred c.raw_value.depends
      ) then
      [ "field `depends` must be an array of claim ids or doc#anchor refs" ]
    else
      [])
    @ (if has_because
      && !(
        std.is_array c.raw_value.because
        && std.array.all claim.RefPred c.raw_value.because
      ) then
      [ "field `because` must be an array of claim ids or doc#anchor refs" ]
    else
      [])
in

let c1_diagnostics = fun claims =>
  std.array.flat_map
    (fun c =>
      let violations = claim_field_violations c in
      if std.array.length violations == 0 then
        []
      else
        [
          {
            check = "C1",
            severity = "fail",
            file = c.file,
            line = c.block_line,
            message = std.array.first violations,
          }
        ]
    )
    claims
in

# --- C2: claim ids are unique corpus-wide. ------------------------------

let c2_diagnostics = fun claims =>
  std.array.flat_map
    (fun c =>
      let others =
        std.array.filter
          (fun o => o.id == c.id && !(o.file == c.file && o.heading_line == c.heading_line))
          claims
      in
      if std.array.length others == 0 then
        []
      else
        [
          {
            check = "C2",
            severity = "fail",
            file = c.file,
            line = c.heading_line,
            message =
              "id \"%{c.id}\" is also declared at %{std.string.join ", " (std.array.map (fun o => "%{o.file}:%{std.string.from_number o.heading_line}") others)}",
          }
        ]
    )
    claims
in

# --- C3: kind is permitted by the genre of the claim's own file. -------

let c3_diagnostics = fun claims documents genres =>
  std.array.flat_map
    (fun c =>
      let docs = std.array.filter (fun d => d.file == c.file) documents in
      if std.array.length docs == 0 then
        []
      else
        let matching_genres =
          std.array.filter (fun g => g.path == (std.array.first docs).genre_path) genres
        in
        if std.array.length matching_genres == 0 then
          []
        else
          # A missing/unrecognized `kind` is C1's finding, not C3's.
          let raw_ok = c.raw_error == null && std.is_record c.raw_value in
          if !raw_ok || !std.array.elem "kind" (std.record.fields c.raw_value) then
            []
          else
            let kind = c.raw_value.kind in
            if !claim.KindPred kind then
              []
            else
              let g = std.array.first matching_genres in
              if std.array.elem kind g.kinds then
                []
              else
                [
                  {
                    check = "C3",
                    severity = "fail",
                    file = c.file,
                    line = c.block_line,
                    message =
                      "kind \"%{kind}\" is not permitted by genre \"%{g.path}\" (permits [%{std.string.join ", " g.kinds}])",
                  }
                ]
    )
    claims
in

# --- C4 / orphaned-because: depends/because targets resolve. -----------
# Dangling `depends` breaks the claim (Fail); dangling `because` only
# orphans its stated reason (Warn) — reference kinds R1.

let refs_check = fun check_id sev field_name broken_word claim_ids documents claims =>
  std.array.flat_map
    (fun c =>
      std.array.flat_map
        (fun raw =>
          let cite = parse_ref raw in
          if resolves claim_ids documents cite then
            []
          else
            [
              {
                # Bound to `sev`, not `severity`: Nickel records are
                # recursive, so a field named `severity` assigned from a
                # same-named outer binding would shadow itself —
                # "infinite recursion", the same defect class
                # `evaluate_impl` above works around, confirmed directly
                # against Nickel 1.17.0 rather than assumed twice.
                check = check_id,
                severity = sev,
                file = c.file,
                line = c.block_line,
                message = "%{field_name} target %{ref_to_string cite} does not resolve — %{broken_word}",
              }
            ]
        )
        (if field_name == "depends" then c.depends else c.because)
    )
    claims
in

let c4_diagnostics = fun claim_ids documents claims =>
  refs_check
    "C4"
    "fail"
    "depends"
    "this claim is broken: rewire or remove the dependency"
    claim_ids
    documents
    claims
in

let orphaned_because_diagnostics = fun claim_ids documents claims =>
  refs_check
    "orphaned-because"
    "warn"
    "because"
    "the claim's stated reason is orphaned, not the claim itself: restate the reason, or confirm the claim was vestigial"
    claim_ids
    documents
    claims
in

# --- C5 (replaced rule, R3): every depends/because entry carries a -----
# prose link; a prose link that is not declared is a bare reference.

let dir_of = fun path =>
  let parts = std.string.split "/" path in
  if std.array.length parts <= 1 then "" else std.string.join "/" (std.array.drop_last parts)
in

let strip_md_extension = fun path =>
  let n = std.string.length path in
  if n >= 3 && std.string.substring (n - 3) n path == ".md" then
    std.string.substring 0 (n - 3) path
  else
    path
in

# Lexically resolve `relative` against `base_dir`; `null` if a `..`
# would pop past the corpus root (MVP.md §1.3).
let resolve_relative = fun base_dir relative =>
  let root_relative =
    std.string.length relative > 0 && std.string.substring 0 1 relative == "/"
  in
  let base_dir_eff = if root_relative then "" else base_dir in
  let relative_eff =
    if root_relative then
      std.string.substring 1 (std.string.length relative) relative
    else
      relative
  in
  let initial =
    if std.string.length base_dir_eff == 0 then [] else std.string.split "/" base_dir_eff
  in
  let result =
    std.array.fold_left
      (fun acc seg =>
        if acc == null then
          null
        else if seg == "" || seg == "." then
          acc
        else if seg == ".." then
          (if std.array.length acc == 0 then null else std.array.drop_last acc)
        else
          acc @ [seg]
      )
      initial
      (std.string.split "/" relative_eff)
  in
  if result == null then null else std.string.join "/" result
in

# Mirrors `checks::normalize_prose_link` exactly, including its four
# cases (fragment-only; bare claim-id candidate; relative path; no
# representation).
let normalize_prose_link = fun href claiming_file =>
  let split = split_first "#" href in
  let path_part = split.head in
  # Bound to `anc`, not `anchor`: a record field named `anchor` assigned
  # from a same-named outer binding would shadow itself under Nickel's
  # recursive-record scoping ("infinite recursion") — the same defect
  # class `refs_check` above works around.
  let anc = if split.tail == null || std.string.length split.tail == 0 then null else split.tail in
  if std.string.length path_part == 0 then
    (if anc == null then null else { tag = 'doc_anchor, path = strip_md_extension claiming_file, anchor = anc })
  else if
    anc == null
    && !std.string.contains "/" path_part
    && !(
      std.string.length path_part >= 3
      && std.string.substring (std.string.length path_part - 3) (std.string.length path_part) path_part
      == ".md"
    )
  then
    { tag = 'claim, id = path_part }
  else
    let resolved = resolve_relative (dir_of claiming_file) path_part in
    if resolved == null || anc == null then
      null
    else
      { tag = 'doc_anchor, path = strip_md_extension resolved, anchor = anc }
in

let is_ref_shaped = fun cite =>
  if cite.tag == 'claim then
    std.string.is_match claim.ClaimIdPattern cite.id
  else
    std.string.length cite.path > 0 && std.string.length cite.anchor > 0
in

# Whether a declared depends/because target is covered by the claim's
# normalized prose-link set (C5's `D ⊆ L`). A claim-id declaration is
# additionally satisfied by an anchor-form prose link whose anchor
# equals the id — see `checks::prose_link_declares`'s doc comment for
# why the bare form stays accepted too.
let prose_link_declares = fun cite prose =>
  std.array.any (fun p => ref_eq cite p) prose
  || (
    cite.tag == 'claim
    && std.array.any (fun p => p.tag == 'doc_anchor && p.anchor == cite.id) prose
  )
in

let c5_diagnostics = fun claims =>
  std.array.flat_map
    (fun c =>
      let normalized =
        std.array.filter
          (fun r => r != null)
          (std.array.map (fun href => normalize_prose_link href c.file) c.prose_links)
      in
      let prose_set = std.array.filter is_ref_shaped normalized in
      let all_refs =
        (std.array.map (fun raw => { kind = "depends", cite = parse_ref raw }) c.depends)
        @ (std.array.map (fun raw => { kind = "because", cite = parse_ref raw }) c.because)
      in
      let undeclared = std.array.filter (fun r => !prose_link_declares r.cite prose_set) all_refs in
      if std.array.length undeclared == 0 then
        []
      else
        [
          {
            check = "C5",
            severity = "fail",
            file = c.file,
            line = c.heading_line,
            message =
              "\"%{c.id}\" declares a dependence or reason with no matching prose link: [%{std.string.join ", " (std.array.map (fun r => "\"%{r.kind}:%{ref_to_string r.cite}\"") undeclared)}]",
          }
        ]
    )
    claims
in

# --- orphan-claim, normative-prose, unregistered-definition: -----------
# these are already-detected facts (extract.rs scans genre-agnostically);
# what's pure here is deciding, given genres now in view, which ones are
# violations, and formatting the diagnostic.

let orphan_claim_diagnostics = fun orphan_claims =>
  std.array.map
    (fun o => {
      check = "orphan-claim",
      severity = "fail",
      file = o.file,
      line = o.line,
      message = "claim block has no preceding bracket-kebab id heading",
    })
    orphan_claims
in

let normative_prose_diagnostics = fun documents genres occurrences =>
  std.array.flat_map
    (fun occ =>
      let docs = std.array.filter (fun d => d.file == occ.file) documents in
      if std.array.length docs == 0 then
        []
      else
        let matching_genres =
          std.array.filter (fun g => g.path == (std.array.first docs).genre_path) genres
        in
        if std.array.length matching_genres == 0 then
          []
        else
          let g = std.array.first matching_genres in
          if std.array.length g.kinds > 0 then
            []
          else
            [
              {
                check = "normative-prose",
                severity = "fail",
                file = occ.file,
                line = occ.line,
                message =
                  "normative keyword \"%{occ.keyword}\" in own-voice prose, but genre \"%{g.path}\" permits no claim kinds",
              }
            ]
    )
    occurrences
in

let unregistered_definition_diagnostics = fun defs =>
  std.array.map
    (fun d => {
      check = "unregistered-definition",
      severity = "warn",
      file = d.file,
      line = d.line,
      message = "definition \"%{d.id}\" has no claim block — unregistered",
    })
    defs
in

# --- malformed-id: a bracketed token in definition position that fails --
# the id grammar (`.ledger/2026-08-04-malformed-ids-are-silently-invisible.md`).
# `Warn` severity, like `unregistered-definition`: real corpora are
# expected to carry some of these on the day this check ships, and the
# remedy is a rename an author makes, not a gate that should block a
# commit already in flight.

let malformed_id_diagnostics = fun ids =>
  std.array.map
    (fun m => {
      check = "malformed-id",
      severity = "warn",
      file = m.file,
      line = m.line,
      message =
        "bracketed token \"%{m.id}\" is in definition position but is not valid kebab-case — rename it (a malformed id is neither claimed nor counted as unregistered)",
    })
    ids
in

# --- the index (MVP.md §4.1): a pure projection. ------------------------

let index_claims = fun claims =>
  std.array.map
    (fun c =>
      let raw_ok = c.raw_error == null && std.is_record c.raw_value in
      {
        id = c.id,
        file = c.file,
        line = c.block_line,
        kind =
          if raw_ok && std.array.elem "kind" (std.record.fields c.raw_value) && std.is_string c.raw_value.kind then
            c.raw_value.kind
          else
            "",
        evaluator =
          if
            raw_ok
            && std.array.elem "evaluator" (std.record.fields c.raw_value)
            && std.is_string c.raw_value.evaluator
          then
            c.raw_value.evaluator
          else
            "",
        depends = c.depends,
        because = c.because,
      }
    )
    claims
in

let index_documents = fun documents =>
  std.array.map (fun d => { doc_path = d.doc_path, file = d.file, genre = d.genre_path }) documents
in

# --- entry point ---------------------------------------------------------

# Bound under a name distinct from the exported field below: Nickel
# records are recursive, so `{ evaluate = evaluate }` would have the
# field shadow this let-binding and refer to itself — "infinite
# recursion", confirmed directly rather than assumed.
let evaluate_impl = fun input =>
  let claim_ids = std.array.map (fun c => c.id) input.claims in
  {
    index = {
      claims = index_claims input.claims,
      documents = index_documents input.documents,
    },
    diagnostics =
      (orphan_claim_diagnostics input.orphan_claims)
      @ (c1_diagnostics input.claims)
      @ (c2_diagnostics input.claims)
      @ (c3_diagnostics input.claims input.documents input.genres)
      @ (c4_diagnostics claim_ids input.documents input.claims)
      @ (orphaned_because_diagnostics claim_ids input.documents input.claims)
      @ (c5_diagnostics input.claims)
      @ (normative_prose_diagnostics input.documents input.genres input.normative_occurrences)
      @ (unregistered_definition_diagnostics input.unregistered_definitions)
      @ (malformed_id_diagnostics input.malformed_ids),
  }
in
{ evaluate = evaluate_impl }
