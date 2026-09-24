//! A deterministic, optimizer-independent check of small proposed AP plans.
//!
//! The caller supplies candidate-to-demand link coefficients and an explicit
//! one-radio-per-demand assignment. This crate recomputes the represented
//! coverage, capacity, client-count, AP-count, and installation-budget facts.
//! It does not generate candidates, solve assignments, model airtime or
//! interference, repair a plan, or establish that a plan is optimal.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const MAX_CANDIDATES: usize = 128;
const MAX_DEMANDS: usize = 4_096;
const MAX_LINK_ESTIMATES: usize = 131_072;
const MAX_ASSIGNMENTS: usize = MAX_DEMANDS;

/// Maximum number of caller-supplied cases admitted to one robust evaluation.
pub const MAX_SCENARIO_CASES: usize = 64;
/// Maximum deterministic aggregate work units admitted across all cases.
///
/// A case accounts for `demands * candidates + demands + candidates + links +
/// selected candidates + assignments`. This bounds both the evaluator's
/// nested demand/candidate scan and its record-processing passes.
pub const MAX_AGGREGATE_SCENARIO_WORK_UNITS: u64 = 1_057_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CandidateId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DemandId(pub u32);

/// RSSI in tenths of a dBm. Integer storage avoids floating-point boundary
/// choices in hard-constraint evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DbmTenths(pub i16);

/// A caller-defined, common integer scale proportional to each demand area's
/// weight. The evaluator does not infer geometry or area from these values.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AreaWeight(pub u64);

/// Decimal kilobits per second, used for caller-supplied offered load and
/// radio capacity coefficients.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct KilobitsPerSecond(pub u64);

/// Integer cents in the caller's chosen currency.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CostCents(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemandCell {
    pub id: DemandId,
    pub area_weight: AreaWeight,
    pub required_distinct_access_points: u16,
    pub minimum_downlink: DbmTenths,
    pub minimum_uplink: DbmTenths,
    pub offered_load: KilobitsPerSecond,
    pub client_count: u32,
}

/// One candidate AP with exactly one modeled radio configuration and
/// precomputed link evidence. Multiple radios or configurations at one
/// physical candidate are outside this contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateAp {
    pub id: CandidateId,
    pub installation_cost: CostCents,
    pub capacity: KilobitsPerSecond,
    pub maximum_clients: u32,
    pub links: Vec<LinkEstimate>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkEstimate {
    pub demand_id: DemandId,
    pub downlink: DbmTenths,
    pub uplink: DbmTenths,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannerProblem {
    pub demands: Vec<DemandCell>,
    pub candidates: Vec<CandidateAp>,
    pub maximum_access_points: u16,
    pub installation_budget: CostCents,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DemandAssignment {
    pub demand_id: DemandId,
    pub candidate_id: CandidateId,
}

/// One whole demand cell is assigned to at most one selected radio. The
/// evaluator validates this supplied assignment; it does not optimize it or
/// split demand among radios.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProposedPlan {
    pub selected_candidates: Vec<CandidateId>,
    pub assignments: Vec<DemandAssignment>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ConstraintKind {
    AccessPointCount,
    InstallationBudget,
    BidirectionalCoverage,
    AssignedDemandLink,
    RadioCapacity,
    RadioClientCount,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ConstraintScope {
    Plan,
    Demand(DemandId),
    Candidate(CandidateId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConstraintStatus {
    Satisfied,
    Binding,
    Violated,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConstraintMeasure {
    Count(u64),
    AreaWeight(AreaWeight),
    Cost(CostCents),
    Throughput(KilobitsPerSecond),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalFailure {
    Downlink,
    Uplink,
    Both,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkDeficit {
    pub candidate_id: CandidateId,
    pub estimate: LinkEstimate,
    pub required_downlink: DbmTenths,
    pub required_uplink: DbmTenths,
    pub failure: SignalFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignmentReason {
    Missing,
    CandidateNotSelected,
    LinkCoefficientUnavailable,
    LinkBelowThreshold,
    LinkMeetsThreshold,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConstraintEvidence {
    PlanLimit,
    Coverage {
        eligible_selected: Vec<CandidateId>,
        eligible_unselected: Vec<CandidateId>,
        unknown_selected: Vec<CandidateId>,
        below_threshold_selected: Vec<LinkDeficit>,
    },
    Assignment {
        reason: AssignmentReason,
        candidate_id: Option<CandidateId>,
        link: Option<LinkEstimate>,
        required_downlink: DbmTenths,
        required_uplink: DbmTenths,
    },
    RadioLoad {
        assigned_demands: Vec<DemandId>,
    },
    RadioClients {
        assigned_demands: Vec<DemandId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConstraintFinding {
    pub kind: ConstraintKind,
    pub scope: ConstraintScope,
    pub status: ConstraintStatus,
    /// The measured/verified input value. The unit is determined by `kind`.
    pub actual: ConstraintMeasure,
    pub limit: ConstraintMeasure,
    /// Structured evidence for a reviewer or UI to render without inventing a
    /// natural-language explanation.
    pub evidence: ConstraintEvidence,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObjectiveBreakdown {
    pub total_area_weight: AreaWeight,
    pub passing_area_weight: AreaWeight,
    pub nonpassing_area_weight: AreaWeight,
    pub unverified_area_weight: AreaWeight,
    pub total_offered_load: KilobitsPerSecond,
    /// Load assigned to a selected AP over a threshold-passing link. Aggregate
    /// AP capacity is checked separately; this component does not claim that
    /// an overloaded AP can serve every assigned demand simultaneously.
    pub served_load: KilobitsPerSecond,
    /// Offered load without a selected, threshold-passing assignment.
    pub unserved_load: KilobitsPerSecond,
    pub capacity_headroom: KilobitsPerSecond,
    pub capacity_overload: KilobitsPerSecond,
    pub client_limit_excess: u64,
    pub installation_cost: CostCents,
    pub access_point_count: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanEvaluation {
    /// False for any violated or unknown hard constraint. Binding constraints
    /// are feasible and are reported explicitly in `findings`.
    pub feasible: bool,
    /// A component vector, not a combined score or an optimizer ranking.
    pub objectives: ObjectiveBreakdown,
    /// Findings are returned in stable plan/demand/candidate order.
    pub findings: Vec<ConstraintFinding>,
}

/// Stable caller-assigned identity for one explicit failure/uncertainty case.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ScenarioId(pub u32);

/// One complete scenario input. The caller supplies the full problem and plan
/// for this case, including any candidate removal, coefficient perturbation,
/// and reassignment. No failure or uncertainty is inferred by this crate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioCase {
    pub id: ScenarioId,
    pub problem: PlannerProblem,
    pub plan: ProposedPlan,
}

/// Exact minimum feasible-case fraction. Zero numerators, zero denominators,
/// and fractions above one are rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RobustnessPolicy {
    pub minimum_feasible_numerator: u32,
    pub minimum_feasible_denominator: u32,
}

impl RobustnessPolicy {
    /// Require every supplied case to pass.
    pub const ALL_CASES: Self = Self {
        minimum_feasible_numerator: 1,
        minimum_feasible_denominator: 1,
    };
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioPlanEvaluation {
    pub scenario_id: ScenarioId,
    pub evaluation: PlanEvaluation,
}

/// Aggregate result for one explicitly supplied scenario set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RobustEvaluation {
    /// Whether the exact rational threshold is met. Individual case results
    /// remain available even when a policy permits some failures.
    pub feasible: bool,
    pub policy: RobustnessPolicy,
    pub feasible_scenario_count: usize,
    pub total_scenario_count: usize,
    pub minimum_feasible_scenario_count: usize,
    /// Sorted by `ScenarioId`, independent of input case order.
    pub scenario_evaluations: Vec<ScenarioPlanEvaluation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvaluationError {
    ResourceLimit(&'static str),
    DuplicateDemand(DemandId),
    DuplicateCandidate(CandidateId),
    DuplicateLink {
        candidate_id: CandidateId,
        demand_id: DemandId,
    },
    UnknownLinkDemand {
        candidate_id: CandidateId,
        demand_id: DemandId,
    },
    ZeroAreaWeight(DemandId),
    ZeroRequiredAccessPoints(DemandId),
    DuplicatePlacement(CandidateId),
    UnknownCandidate(CandidateId),
    DuplicateAssignment(DemandId),
    UnknownDemand(DemandId),
    ArithmeticOverflow(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScenarioEvaluationError {
    EmptyScenarioSet,
    DuplicateScenarioId(ScenarioId),
    InvalidRobustnessPolicy {
        numerator: u32,
        denominator: u32,
    },
    ResourceLimit(&'static str),
    CaseEvaluation {
        scenario_id: ScenarioId,
        error: EvaluationError,
    },
}

impl fmt::Display for ScenarioEvaluationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ScenarioEvaluationError {}

impl fmt::Display for EvaluationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for EvaluationError {}

/// Evaluate an explicit set of complete plan cases and report whether the
/// caller's exact rational feasibility policy is met. Cases are evaluated and
/// returned in ascending ID order.
///
/// Scenario IDs, per-case resource caps, the scenario-count cap, and aggregate
/// work budget are checked before any plan is evaluated. A malformed case or
/// evaluator error returns only `Err`; no partial aggregate is observable.
/// Numeric policy decisions use integer cross-multiplication, never floating point.
/// This function neither generates N-1 cases nor infers uncertainty,
/// reassigns demand, optimizes, or repairs plans.
pub fn evaluate_scenarios(
    cases: &[ScenarioCase],
    policy: RobustnessPolicy,
) -> Result<RobustEvaluation, ScenarioEvaluationError> {
    if policy.minimum_feasible_denominator == 0
        || policy.minimum_feasible_numerator == 0
        || policy.minimum_feasible_numerator > policy.minimum_feasible_denominator
    {
        return Err(ScenarioEvaluationError::InvalidRobustnessPolicy {
            numerator: policy.minimum_feasible_numerator,
            denominator: policy.minimum_feasible_denominator,
        });
    }
    if cases.is_empty() {
        return Err(ScenarioEvaluationError::EmptyScenarioSet);
    }
    if cases.len() > MAX_SCENARIO_CASES {
        return Err(ScenarioEvaluationError::ResourceLimit("scenario count"));
    }

    let mut ordered_cases: Vec<_> = cases.iter().collect();
    ordered_cases.sort_by_key(|case| case.id);
    for pair in ordered_cases.windows(2) {
        if pair[0].id == pair[1].id {
            return Err(ScenarioEvaluationError::DuplicateScenarioId(pair[0].id));
        }
    }

    let mut aggregate_work_units = 0_u64;
    for case in &ordered_cases {
        let case_work_units =
            scenario_work_units(case).map_err(|error| ScenarioEvaluationError::CaseEvaluation {
                scenario_id: case.id,
                error,
            })?;
        aggregate_work_units = aggregate_work_units.checked_add(case_work_units).ok_or(
            ScenarioEvaluationError::ResourceLimit("aggregate scenario work overflow"),
        )?;
        if aggregate_work_units > MAX_AGGREGATE_SCENARIO_WORK_UNITS {
            return Err(ScenarioEvaluationError::ResourceLimit(
                "aggregate scenario work",
            ));
        }
    }

    let mut scenario_evaluations = Vec::with_capacity(ordered_cases.len());
    let mut feasible_scenario_count = 0_usize;
    for case in ordered_cases {
        let evaluation = evaluate(&case.problem, &case.plan).map_err(|error| {
            ScenarioEvaluationError::CaseEvaluation {
                scenario_id: case.id,
                error,
            }
        })?;
        feasible_scenario_count += usize::from(evaluation.feasible);
        scenario_evaluations.push(ScenarioPlanEvaluation {
            scenario_id: case.id,
            evaluation,
        });
    }

    let scenario_count = scenario_evaluations.len();
    let threshold_numerator = u128::from(policy.minimum_feasible_numerator);
    let threshold_denominator = u128::from(policy.minimum_feasible_denominator);
    let required_product = u128::try_from(scenario_count)
        .expect("scenario cap fits u128")
        .checked_mul(threshold_numerator)
        .expect("bounded scenario threshold product fits u128");
    let minimum_feasible_scenario_count = usize::try_from(
        required_product / threshold_denominator
            + u128::from(required_product % threshold_denominator != 0),
    )
    .expect("minimum required scenario count is at most the scenario count");
    let meets_policy = u128::try_from(feasible_scenario_count)
        .expect("scenario cap fits u128")
        .checked_mul(threshold_denominator)
        .expect("bounded scenario threshold product fits u128")
        >= u128::try_from(scenario_count)
            .expect("scenario cap fits u128")
            .checked_mul(threshold_numerator)
            .expect("bounded scenario threshold product fits u128");

    Ok(RobustEvaluation {
        feasible: meets_policy,
        policy,
        feasible_scenario_count,
        total_scenario_count: scenario_count,
        minimum_feasible_scenario_count,
        scenario_evaluations,
    })
}

fn scenario_work_units(case: &ScenarioCase) -> Result<u64, EvaluationError> {
    let problem = &case.problem;
    let link_count = bounded_input_link_count(problem, &case.plan)?;

    let demands = u64::try_from(problem.demands.len())
        .map_err(|_| EvaluationError::ResourceLimit("scenario demand count overflow"))?;
    let candidates = u64::try_from(problem.candidates.len())
        .map_err(|_| EvaluationError::ResourceLimit("scenario candidate count overflow"))?;
    let links = u64::try_from(link_count)
        .map_err(|_| EvaluationError::ResourceLimit("scenario link count overflow"))?;
    let selected = u64::try_from(case.plan.selected_candidates.len())
        .map_err(|_| EvaluationError::ResourceLimit("scenario selection count overflow"))?;
    let assignments = u64::try_from(case.plan.assignments.len())
        .map_err(|_| EvaluationError::ResourceLimit("scenario assignment count overflow"))?;

    demands
        .checked_mul(candidates)
        .and_then(|units| units.checked_add(demands))
        .and_then(|units| units.checked_add(candidates))
        .and_then(|units| units.checked_add(links))
        .and_then(|units| units.checked_add(selected))
        .and_then(|units| units.checked_add(assignments))
        .ok_or(EvaluationError::ResourceLimit(
            "scenario work estimate overflow",
        ))
}

fn bounded_input_link_count(
    problem: &PlannerProblem,
    plan: &ProposedPlan,
) -> Result<usize, EvaluationError> {
    if problem.candidates.len() > MAX_CANDIDATES {
        return Err(EvaluationError::ResourceLimit("candidate count"));
    }
    if problem.demands.len() > MAX_DEMANDS {
        return Err(EvaluationError::ResourceLimit("demand count"));
    }
    if plan.selected_candidates.len() > MAX_CANDIDATES {
        return Err(EvaluationError::ResourceLimit("selected candidate count"));
    }
    if plan.assignments.len() > MAX_ASSIGNMENTS {
        return Err(EvaluationError::ResourceLimit("assignment count"));
    }

    let link_count = problem
        .candidates
        .iter()
        .try_fold(0usize, |count, candidate| {
            count
                .checked_add(candidate.links.len())
                .ok_or(EvaluationError::ResourceLimit(
                    "link coefficient count overflow",
                ))
        })?;
    if link_count > MAX_LINK_ESTIMATES {
        return Err(EvaluationError::ResourceLimit("link estimate count"));
    }
    Ok(link_count)
}

/// Recompute represented hard constraints and objective components for a
/// proposed plan. Missing selected-candidate link coefficients are `Unknown`
/// and fail closed when they could change a coverage result.
pub fn evaluate(
    problem: &PlannerProblem,
    plan: &ProposedPlan,
) -> Result<PlanEvaluation, EvaluationError> {
    bounded_input_link_count(problem, plan)?;

    let mut demands = BTreeMap::new();
    for demand in &problem.demands {
        if demand.area_weight.0 == 0 {
            return Err(EvaluationError::ZeroAreaWeight(demand.id));
        }
        if demand.required_distinct_access_points == 0 {
            return Err(EvaluationError::ZeroRequiredAccessPoints(demand.id));
        }
        if demands.insert(demand.id, demand).is_some() {
            return Err(EvaluationError::DuplicateDemand(demand.id));
        }
    }

    let mut candidates = BTreeMap::new();
    let mut links = BTreeMap::new();
    for candidate in &problem.candidates {
        if candidates.insert(candidate.id, candidate).is_some() {
            return Err(EvaluationError::DuplicateCandidate(candidate.id));
        }
        for link in &candidate.links {
            if !demands.contains_key(&link.demand_id) {
                return Err(EvaluationError::UnknownLinkDemand {
                    candidate_id: candidate.id,
                    demand_id: link.demand_id,
                });
            }
            if links.insert((candidate.id, link.demand_id), link).is_some() {
                return Err(EvaluationError::DuplicateLink {
                    candidate_id: candidate.id,
                    demand_id: link.demand_id,
                });
            }
        }
    }

    let mut selected = BTreeSet::new();
    for candidate_id in &plan.selected_candidates {
        if !candidates.contains_key(candidate_id) {
            return Err(EvaluationError::UnknownCandidate(*candidate_id));
        }
        if !selected.insert(*candidate_id) {
            return Err(EvaluationError::DuplicatePlacement(*candidate_id));
        }
    }

    let mut assignments = BTreeMap::new();
    for assignment in &plan.assignments {
        if !demands.contains_key(&assignment.demand_id) {
            return Err(EvaluationError::UnknownDemand(assignment.demand_id));
        }
        if !candidates.contains_key(&assignment.candidate_id) {
            return Err(EvaluationError::UnknownCandidate(assignment.candidate_id));
        }
        if assignments
            .insert(assignment.demand_id, assignment.candidate_id)
            .is_some()
        {
            return Err(EvaluationError::DuplicateAssignment(assignment.demand_id));
        }
    }

    let mut findings = Vec::new();
    let selected_count = u64::try_from(selected.len())
        .map_err(|_| EvaluationError::ArithmeticOverflow("selected AP count"))?;
    findings.push(ConstraintFinding {
        kind: ConstraintKind::AccessPointCount,
        scope: ConstraintScope::Plan,
        status: upper_bound_status(selected_count, u64::from(problem.maximum_access_points)),
        actual: ConstraintMeasure::Count(selected_count),
        limit: ConstraintMeasure::Count(u64::from(problem.maximum_access_points)),
        evidence: ConstraintEvidence::PlanLimit,
    });

    let mut objectives = ObjectiveBreakdown {
        access_point_count: u16::try_from(selected.len())
            .map_err(|_| EvaluationError::ArithmeticOverflow("selected AP count"))?,
        ..ObjectiveBreakdown::default()
    };
    for candidate_id in &selected {
        let candidate = candidates[candidate_id];
        objectives.installation_cost = CostCents(checked_add(
            objectives.installation_cost.0,
            candidate.installation_cost.0,
            "installation cost",
        )?);
    }
    findings.push(ConstraintFinding {
        kind: ConstraintKind::InstallationBudget,
        scope: ConstraintScope::Plan,
        status: upper_bound_status(
            objectives.installation_cost.0,
            problem.installation_budget.0,
        ),
        actual: ConstraintMeasure::Cost(objectives.installation_cost),
        limit: ConstraintMeasure::Cost(problem.installation_budget),
        evidence: ConstraintEvidence::PlanLimit,
    });

    let mut loads = BTreeMap::<CandidateId, KilobitsPerSecond>::new();
    let mut client_loads = BTreeMap::<CandidateId, u64>::new();
    let mut assigned_demands = BTreeMap::<CandidateId, Vec<DemandId>>::new();
    for candidate_id in &selected {
        loads.insert(*candidate_id, KilobitsPerSecond(0));
        client_loads.insert(*candidate_id, 0);
        assigned_demands.insert(*candidate_id, Vec::new());
    }

    let mut coverage_status = BTreeMap::<DemandId, ConstraintStatus>::new();
    let mut assignment_status = BTreeMap::<DemandId, ConstraintStatus>::new();
    for (demand_id, demand) in &demands {
        objectives.total_area_weight = AreaWeight(checked_add(
            objectives.total_area_weight.0,
            demand.area_weight.0,
            "total area weight",
        )?);
        objectives.total_offered_load = KilobitsPerSecond(checked_add(
            objectives.total_offered_load.0,
            demand.offered_load.0,
            "total offered load",
        )?);

        let mut eligible_selected = Vec::new();
        let mut eligible_unselected = Vec::new();
        let mut unknown_selected = Vec::new();
        let mut below_threshold_selected = Vec::new();
        for candidate_id in candidates.keys() {
            match links.get(&(*candidate_id, *demand_id)) {
                Some(link) if meets_threshold(link, demand) => {
                    if selected.contains(candidate_id) {
                        eligible_selected.push(*candidate_id);
                    } else {
                        eligible_unselected.push(*candidate_id);
                    }
                }
                Some(link) if selected.contains(candidate_id) => {
                    below_threshold_selected.push(link_deficit(*candidate_id, link, demand));
                }
                None if selected.contains(candidate_id) => {
                    unknown_selected.push(*candidate_id);
                }
                Some(_) | None => {}
            }
        }
        let eligible_count = u64::try_from(eligible_selected.len())
            .map_err(|_| EvaluationError::ArithmeticOverflow("eligible radio count"))?;
        let required = u64::from(demand.required_distinct_access_points);
        let possible_count = eligible_count
            .checked_add(
                u64::try_from(unknown_selected.len())
                    .map_err(|_| EvaluationError::ArithmeticOverflow("unknown radio count"))?,
            )
            .ok_or(EvaluationError::ArithmeticOverflow("possible radio count"))?;
        let coverage_state = if eligible_count >= required {
            if eligible_count == required {
                ConstraintStatus::Binding
            } else {
                ConstraintStatus::Satisfied
            }
        } else if possible_count >= required {
            ConstraintStatus::Unknown
        } else {
            ConstraintStatus::Violated
        };
        coverage_status.insert(*demand_id, coverage_state);
        if matches!(
            coverage_state,
            ConstraintStatus::Satisfied | ConstraintStatus::Binding
        ) {
            objectives.passing_area_weight = AreaWeight(checked_add(
                objectives.passing_area_weight.0,
                demand.area_weight.0,
                "passing area weight",
            )?);
        } else {
            objectives.nonpassing_area_weight = AreaWeight(checked_add(
                objectives.nonpassing_area_weight.0,
                demand.area_weight.0,
                "nonpassing area weight",
            )?);
            if coverage_state == ConstraintStatus::Unknown {
                objectives.unverified_area_weight = AreaWeight(checked_add(
                    objectives.unverified_area_weight.0,
                    demand.area_weight.0,
                    "unverified area weight",
                )?);
            }
        }
        findings.push(ConstraintFinding {
            kind: ConstraintKind::BidirectionalCoverage,
            scope: ConstraintScope::Demand(*demand_id),
            status: coverage_state,
            actual: ConstraintMeasure::Count(eligible_count),
            limit: ConstraintMeasure::Count(required),
            evidence: ConstraintEvidence::Coverage {
                eligible_selected,
                eligible_unselected,
                unknown_selected,
                below_threshold_selected,
            },
        });

        let assigned_candidate = assignments.get(demand_id).copied();
        let (assignment_state, reason, link) = match assigned_candidate {
            None => (ConstraintStatus::Violated, AssignmentReason::Missing, None),
            Some(candidate_id) if !selected.contains(&candidate_id) => (
                ConstraintStatus::Violated,
                AssignmentReason::CandidateNotSelected,
                links.get(&(candidate_id, *demand_id)).copied().copied(),
            ),
            Some(candidate_id) => match links.get(&(candidate_id, *demand_id)).copied() {
                None => (
                    ConstraintStatus::Unknown,
                    AssignmentReason::LinkCoefficientUnavailable,
                    None,
                ),
                Some(link) if !meets_threshold(link, demand) => (
                    ConstraintStatus::Violated,
                    AssignmentReason::LinkBelowThreshold,
                    Some(*link),
                ),
                Some(link) => {
                    let binding = link.downlink == demand.minimum_downlink
                        || link.uplink == demand.minimum_uplink;
                    (
                        if binding {
                            ConstraintStatus::Binding
                        } else {
                            ConstraintStatus::Satisfied
                        },
                        AssignmentReason::LinkMeetsThreshold,
                        Some(*link),
                    )
                }
            },
        };
        assignment_status.insert(*demand_id, assignment_state);
        let served = matches!(
            assignment_state,
            ConstraintStatus::Satisfied | ConstraintStatus::Binding
        );
        if served {
            objectives.served_load = KilobitsPerSecond(checked_add(
                objectives.served_load.0,
                demand.offered_load.0,
                "served load",
            )?);
        } else {
            objectives.unserved_load = KilobitsPerSecond(checked_add(
                objectives.unserved_load.0,
                demand.offered_load.0,
                "unserved load",
            )?);
        }

        if let Some(candidate_id) = assigned_candidate.filter(|id| selected.contains(id)) {
            let load = loads
                .get_mut(&candidate_id)
                .expect("selected load initialized");
            load.0 = checked_add(load.0, demand.offered_load.0, "radio assigned load")?;
            let clients = client_loads
                .get_mut(&candidate_id)
                .expect("selected client load initialized");
            *clients = checked_add(
                *clients,
                u64::from(demand.client_count),
                "radio client count",
            )?;
            assigned_demands
                .get_mut(&candidate_id)
                .expect("selected demand list initialized")
                .push(*demand_id);
        }

        findings.push(ConstraintFinding {
            kind: ConstraintKind::AssignedDemandLink,
            scope: ConstraintScope::Demand(*demand_id),
            status: assignment_state,
            actual: ConstraintMeasure::Count(if served { 1 } else { 0 }),
            limit: ConstraintMeasure::Count(1),
            evidence: ConstraintEvidence::Assignment {
                reason,
                candidate_id: assigned_candidate,
                link,
                required_downlink: demand.minimum_downlink,
                required_uplink: demand.minimum_uplink,
            },
        });
    }

    for candidate_id in &selected {
        let candidate = candidates[candidate_id];
        let load = loads[candidate_id];
        let clients = client_loads[candidate_id];
        let demand_ids = assigned_demands
            .get(candidate_id)
            .expect("selected list initialized");
        objectives.capacity_headroom = KilobitsPerSecond(checked_add(
            objectives.capacity_headroom.0,
            candidate.capacity.0.saturating_sub(load.0),
            "capacity headroom",
        )?);
        objectives.capacity_overload = KilobitsPerSecond(checked_add(
            objectives.capacity_overload.0,
            load.0.saturating_sub(candidate.capacity.0),
            "capacity overload",
        )?);
        objectives.client_limit_excess = checked_add(
            objectives.client_limit_excess,
            clients.saturating_sub(u64::from(candidate.maximum_clients)),
            "client limit excess",
        )?;
        findings.push(ConstraintFinding {
            kind: ConstraintKind::RadioCapacity,
            scope: ConstraintScope::Candidate(*candidate_id),
            status: upper_bound_status(load.0, candidate.capacity.0),
            actual: ConstraintMeasure::Throughput(load),
            limit: ConstraintMeasure::Throughput(candidate.capacity),
            evidence: ConstraintEvidence::RadioLoad {
                assigned_demands: demand_ids.clone(),
            },
        });
        findings.push(ConstraintFinding {
            kind: ConstraintKind::RadioClientCount,
            scope: ConstraintScope::Candidate(*candidate_id),
            status: upper_bound_status(clients, u64::from(candidate.maximum_clients)),
            actual: ConstraintMeasure::Count(clients),
            limit: ConstraintMeasure::Count(u64::from(candidate.maximum_clients)),
            evidence: ConstraintEvidence::RadioClients {
                assigned_demands: demand_ids.clone(),
            },
        });
    }

    let mut feasible = true;
    for finding in &findings {
        if matches!(
            finding.status,
            ConstraintStatus::Violated | ConstraintStatus::Unknown
        ) {
            feasible = false;
            break;
        }
    }

    // The loops above append plan constraints first, then demand findings in
    // ID order, then radio constraints in candidate ID order.
    debug_assert_eq!(coverage_status.len(), demands.len());
    debug_assert_eq!(assignment_status.len(), demands.len());

    Ok(PlanEvaluation {
        feasible,
        objectives,
        findings,
    })
}

fn upper_bound_status(actual: u64, limit: u64) -> ConstraintStatus {
    if actual > limit {
        ConstraintStatus::Violated
    } else if actual == limit {
        ConstraintStatus::Binding
    } else {
        ConstraintStatus::Satisfied
    }
}

fn meets_threshold(link: &LinkEstimate, demand: &DemandCell) -> bool {
    link.downlink >= demand.minimum_downlink && link.uplink >= demand.minimum_uplink
}

fn link_deficit(
    candidate_id: CandidateId,
    link: &LinkEstimate,
    demand: &DemandCell,
) -> LinkDeficit {
    let downlink_fails = link.downlink < demand.minimum_downlink;
    let uplink_fails = link.uplink < demand.minimum_uplink;
    LinkDeficit {
        candidate_id,
        estimate: *link,
        required_downlink: demand.minimum_downlink,
        required_uplink: demand.minimum_uplink,
        failure: match (downlink_fails, uplink_fails) {
            (true, true) => SignalFailure::Both,
            (true, false) => SignalFailure::Downlink,
            (false, true) => SignalFailure::Uplink,
            (false, false) => unreachable!("link deficit requires a threshold failure"),
        },
    }
}

fn checked_add(left: u64, right: u64, name: &'static str) -> Result<u64, EvaluationError> {
    left.checked_add(right)
        .ok_or(EvaluationError::ArithmeticOverflow(name))
}
