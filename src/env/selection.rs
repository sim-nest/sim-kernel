pub(super) struct ScoredCandidate<T> {
    pub(super) cost: u32,
    pub(super) candidate: T,
}

pub(super) enum CandidateSelection<T> {
    None,
    Unique(T),
    Ambiguous(Vec<T>),
}

pub(super) fn choose_best_candidate<T>(
    candidates: impl IntoIterator<Item = ScoredCandidate<T>>,
) -> CandidateSelection<T> {
    let mut best_cost: Option<u32> = None;
    let mut best = Vec::new();

    for candidate in candidates {
        match best_cost {
            None => {
                best_cost = Some(candidate.cost);
                best.push(candidate.candidate);
            }
            Some(cost) if candidate.cost < cost => {
                best_cost = Some(candidate.cost);
                best.clear();
                best.push(candidate.candidate);
            }
            Some(cost) if candidate.cost == cost => {
                best.push(candidate.candidate);
            }
            Some(_) => {}
        }
    }

    match best.len() {
        0 => CandidateSelection::None,
        1 => CandidateSelection::Unique(best.pop().expect("len checked above")),
        _ => CandidateSelection::Ambiguous(best),
    }
}
