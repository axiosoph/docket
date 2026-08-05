// `type` is discharged by existence, not execution — no `:: <command>`
// half, and none is needed: `docket run czd-oid-disjoint` passes on
// locating this marker, without spawning anything (run.rs).
// @docket: czd-oid-disjoint
pub struct Czd<T>(std::marker::PhantomData<T>);
