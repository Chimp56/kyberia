use kyberia_planner_evaluator::{
    AreaWeight, AssignmentReason, CandidateAp, CandidateId, ConstraintEvidence, ConstraintKind,
    ConstraintMeasure, ConstraintScope, ConstraintStatus, CostCents, DbmTenths, DemandAssignment,
    DemandCell, DemandId, EvaluationError, KilobitsPerSecond, LinkEstimate, PlannerProblem,
    ProposedPlan, SignalFailure, evaluate,
};

fn dbm(value: i16) -> DbmTenths {
    DbmTenths(value)
}

fn kbps(value: u64) -> KilobitsPerSecond {
    KilobitsPerSecond(value)
}

fn demand(id: u32, area: u64, load: u64, clients: u32) -> DemandCell {
    DemandCell {
        id: DemandId(id),
        area_weight: AreaWeight(area),
        required_distinct_access_points: 1,
        minimum_downlink: dbm(-650),
        minimum_uplink: dbm(-680),
        offered_load: kbps(load),
        client_count: clients,
    }
}

fn candidate(
    id: u32,
    cost: u64,
    capacity: u64,
    clients: u32,
    links: Vec<LinkEstimate>,
) -> CandidateAp {
    CandidateAp {
        id: CandidateId(id),
        installation_cost: CostCents(cost),
        capacity: kbps(capacity),
        maximum_clients: clients,
        links,
    }
}

fn link(demand_id: u32, downlink: i16, uplink: i16) -> LinkEstimate {
    LinkEstimate {
        demand_id: DemandId(demand_id),
        downlink: dbm(downlink),
        uplink: dbm(uplink),
    }
}

fn problem(
    demands: Vec<DemandCell>,
    candidates: Vec<CandidateAp>,
    max_aps: u16,
    budget: u64,
) -> PlannerProblem {
    PlannerProblem {
        demands,
        candidates,
        maximum_access_points: max_aps,
        installation_budget: CostCents(budget),
    }
}

fn finding(
    evaluation: &kyberia_planner_evaluator::PlanEvaluation,
    kind: ConstraintKind,
    scope: ConstraintScope,
) -> &kyberia_planner_evaluator::ConstraintFinding {
    evaluation
        .findings
        .iter()
        .find(|finding| finding.kind == kind && finding.scope == scope)
        .expect("expected constraint finding")
}

#[test]
fn feasible_small_plan_recomputes_metrics_and_reports_equalities_as_binding() {
    let problem = problem(
        vec![demand(1, 7, 30, 2)],
        vec![candidate(10, 100, 30, 2, vec![link(1, -640, -670)])],
        1,
        100,
    );
    let plan = ProposedPlan {
        selected_candidates: vec![CandidateId(10)],
        assignments: vec![DemandAssignment {
            demand_id: DemandId(1),
            candidate_id: CandidateId(10),
        }],
    };

    let result = evaluate(&problem, &plan).unwrap();

    assert!(result.feasible);
    assert_eq!(result.objectives.total_area_weight, AreaWeight(7));
    assert_eq!(result.objectives.passing_area_weight, AreaWeight(7));
    assert_eq!(result.objectives.nonpassing_area_weight, AreaWeight(0));
    assert_eq!(result.objectives.total_offered_load, kbps(30));
    assert_eq!(result.objectives.served_load, kbps(30));
    assert_eq!(result.objectives.unserved_load, kbps(0));
    assert_eq!(result.objectives.capacity_headroom, kbps(0));
    assert_eq!(result.objectives.capacity_overload, kbps(0));
    assert_eq!(result.objectives.client_limit_excess, 0);
    assert_eq!(result.objectives.installation_cost, CostCents(100));
    assert_eq!(result.objectives.access_point_count, 1);
    assert_eq!(
        finding(
            &result,
            ConstraintKind::AccessPointCount,
            ConstraintScope::Plan
        )
        .status,
        ConstraintStatus::Binding
    );
    assert_eq!(
        finding(
            &result,
            ConstraintKind::InstallationBudget,
            ConstraintScope::Plan
        )
        .status,
        ConstraintStatus::Binding
    );
    assert_eq!(
        finding(
            &result,
            ConstraintKind::BidirectionalCoverage,
            ConstraintScope::Demand(DemandId(1))
        )
        .status,
        ConstraintStatus::Binding
    );
    assert_eq!(
        finding(
            &result,
            ConstraintKind::AssignedDemandLink,
            ConstraintScope::Demand(DemandId(1))
        )
        .status,
        ConstraintStatus::Satisfied
    );
    assert_eq!(
        finding(
            &result,
            ConstraintKind::RadioCapacity,
            ConstraintScope::Candidate(CandidateId(10))
        )
        .status,
        ConstraintStatus::Binding
    );
    assert_eq!(
        finding(
            &result,
            ConstraintKind::RadioClientCount,
            ConstraintScope::Candidate(CandidateId(10))
        )
        .status,
        ConstraintStatus::Binding
    );
}

#[test]
fn bidirectional_coverage_failure_explains_uplink_deficit_and_available_alternative() {
    let mut demand = demand(4, 9, 10, 1);
    demand.required_distinct_access_points = 2;
    let problem = problem(
        vec![demand],
        vec![
            candidate(1, 5, 100, 3, vec![link(4, -640, -700)]),
            candidate(2, 5, 100, 3, vec![link(4, -640, -670)]),
        ],
        1,
        10,
    );
    let plan = ProposedPlan {
        selected_candidates: vec![CandidateId(1)],
        assignments: vec![DemandAssignment {
            demand_id: DemandId(4),
            candidate_id: CandidateId(1),
        }],
    };

    let result = evaluate(&problem, &plan).unwrap();

    assert!(!result.feasible);
    let coverage = finding(
        &result,
        ConstraintKind::BidirectionalCoverage,
        ConstraintScope::Demand(DemandId(4)),
    );
    assert_eq!(coverage.status, ConstraintStatus::Violated);
    match &coverage.evidence {
        ConstraintEvidence::Coverage {
            eligible_selected,
            eligible_unselected,
            below_threshold_selected,
            ..
        } => {
            assert!(eligible_selected.is_empty());
            assert_eq!(eligible_unselected, &vec![CandidateId(2)]);
            assert_eq!(below_threshold_selected.len(), 1);
            assert_eq!(below_threshold_selected[0].failure, SignalFailure::Uplink);
            assert_eq!(below_threshold_selected[0].estimate, link(4, -640, -700));
        }
        evidence => panic!("unexpected evidence: {evidence:?}"),
    }
    let assignment = finding(
        &result,
        ConstraintKind::AssignedDemandLink,
        ConstraintScope::Demand(DemandId(4)),
    );
    assert_eq!(assignment.status, ConstraintStatus::Violated);
    assert!(matches!(
        assignment.evidence,
        ConstraintEvidence::Assignment {
            reason: AssignmentReason::LinkBelowThreshold,
            ..
        }
    ));
}

#[test]
fn missing_selected_link_is_unknown_and_fails_closed() {
    let problem = problem(
        vec![demand(8, 11, 17, 2)],
        vec![candidate(3, 10, 100, 10, vec![])],
        1,
        10,
    );
    let plan = ProposedPlan {
        selected_candidates: vec![CandidateId(3)],
        assignments: vec![DemandAssignment {
            demand_id: DemandId(8),
            candidate_id: CandidateId(3),
        }],
    };

    let result = evaluate(&problem, &plan).unwrap();

    assert!(!result.feasible);
    assert_eq!(result.objectives.unverified_area_weight, AreaWeight(11));
    assert_eq!(result.objectives.nonpassing_area_weight, AreaWeight(11));
    assert_eq!(
        finding(
            &result,
            ConstraintKind::BidirectionalCoverage,
            ConstraintScope::Demand(DemandId(8)),
        )
        .status,
        ConstraintStatus::Unknown
    );
    let assignment = finding(
        &result,
        ConstraintKind::AssignedDemandLink,
        ConstraintScope::Demand(DemandId(8)),
    );
    assert_eq!(assignment.status, ConstraintStatus::Unknown);
    assert!(matches!(
        assignment.evidence,
        ConstraintEvidence::Assignment {
            reason: AssignmentReason::LinkCoefficientUnavailable,
            ..
        }
    ));
}

#[test]
fn capacity_clients_ap_count_and_budget_violations_are_independent() {
    let problem = problem(
        vec![demand(1, 1, 70, 3), demand(2, 1, 70, 3)],
        vec![candidate(
            1,
            100,
            100,
            4,
            vec![link(1, -600, -600), link(2, -600, -600)],
        )],
        0,
        99,
    );
    let plan = ProposedPlan {
        selected_candidates: vec![CandidateId(1)],
        assignments: vec![
            DemandAssignment {
                demand_id: DemandId(1),
                candidate_id: CandidateId(1),
            },
            DemandAssignment {
                demand_id: DemandId(2),
                candidate_id: CandidateId(1),
            },
        ],
    };

    let result = evaluate(&problem, &plan).unwrap();

    assert!(!result.feasible);
    assert_eq!(result.objectives.passing_area_weight, AreaWeight(2));
    assert_eq!(result.objectives.served_load, kbps(140));
    assert_eq!(result.objectives.capacity_headroom, kbps(0));
    assert_eq!(result.objectives.capacity_overload, kbps(40));
    assert_eq!(result.objectives.client_limit_excess, 2);
    assert_eq!(
        finding(
            &result,
            ConstraintKind::AccessPointCount,
            ConstraintScope::Plan
        )
        .status,
        ConstraintStatus::Violated
    );
    assert_eq!(
        finding(
            &result,
            ConstraintKind::InstallationBudget,
            ConstraintScope::Plan
        )
        .status,
        ConstraintStatus::Violated
    );
    assert_eq!(
        finding(
            &result,
            ConstraintKind::RadioCapacity,
            ConstraintScope::Candidate(CandidateId(1))
        )
        .status,
        ConstraintStatus::Violated
    );
    assert_eq!(
        finding(
            &result,
            ConstraintKind::RadioClientCount,
            ConstraintScope::Candidate(CandidateId(1))
        )
        .status,
        ConstraintStatus::Violated
    );
}

#[test]
fn input_permutations_do_not_change_findings_or_objectives() {
    let forward = problem(
        vec![demand(1, 3, 10, 1), demand(2, 5, 20, 2)],
        vec![
            candidate(1, 7, 50, 3, vec![link(1, -600, -600), link(2, -600, -600)]),
            candidate(2, 9, 60, 3, vec![link(1, -610, -610), link(2, -610, -610)]),
        ],
        2,
        20,
    );
    let reversed = problem(
        vec![demand(2, 5, 20, 2), demand(1, 3, 10, 1)],
        vec![
            candidate(2, 9, 60, 3, vec![link(2, -610, -610), link(1, -610, -610)]),
            candidate(1, 7, 50, 3, vec![link(2, -600, -600), link(1, -600, -600)]),
        ],
        2,
        20,
    );
    let plan = ProposedPlan {
        selected_candidates: vec![CandidateId(1), CandidateId(2)],
        assignments: vec![
            DemandAssignment {
                demand_id: DemandId(1),
                candidate_id: CandidateId(1),
            },
            DemandAssignment {
                demand_id: DemandId(2),
                candidate_id: CandidateId(2),
            },
        ],
    };
    let reversed_plan = ProposedPlan {
        selected_candidates: plan.selected_candidates.iter().rev().copied().collect(),
        assignments: plan.assignments.iter().rev().copied().collect(),
    };

    assert_eq!(
        evaluate(&forward, &plan).unwrap(),
        evaluate(&reversed, &reversed_plan).unwrap()
    );
}

#[test]
fn malformed_and_overflowing_inputs_are_rejected() {
    let duplicate_assignment = problem(
        vec![demand(1, 1, 1, 1)],
        vec![candidate(1, 1, 1, 1, vec![link(1, -600, -600)])],
        1,
        1,
    );
    let duplicate_plan = ProposedPlan {
        selected_candidates: vec![CandidateId(1)],
        assignments: vec![
            DemandAssignment {
                demand_id: DemandId(1),
                candidate_id: CandidateId(1),
            },
            DemandAssignment {
                demand_id: DemandId(1),
                candidate_id: CandidateId(1),
            },
        ],
    };
    assert_eq!(
        evaluate(&duplicate_assignment, &duplicate_plan),
        Err(EvaluationError::DuplicateAssignment(DemandId(1)))
    );

    assert_eq!(
        evaluate(
            &problem(vec![demand(1, 1, 1, 1), demand(1, 2, 1, 1)], vec![], 0, 0),
            &ProposedPlan::default()
        ),
        Err(EvaluationError::DuplicateDemand(DemandId(1)))
    );
    assert_eq!(
        evaluate(
            &problem(
                vec![],
                vec![candidate(1, 0, 0, 0, vec![]), candidate(1, 0, 0, 0, vec![])],
                0,
                0
            ),
            &ProposedPlan::default()
        ),
        Err(EvaluationError::DuplicateCandidate(CandidateId(1)))
    );
    assert_eq!(
        evaluate(
            &problem(
                vec![demand(1, 1, 0, 0)],
                vec![candidate(
                    1,
                    0,
                    0,
                    0,
                    vec![link(1, -600, -600), link(1, -600, -600)]
                )],
                0,
                0
            ),
            &ProposedPlan::default()
        ),
        Err(EvaluationError::DuplicateLink {
            candidate_id: CandidateId(1),
            demand_id: DemandId(1)
        })
    );

    let one_candidate = problem(
        vec![demand(1, 1, 0, 0)],
        vec![candidate(1, 0, 0, 0, vec![link(1, -600, -600)])],
        1,
        0,
    );
    assert_eq!(
        evaluate(
            &one_candidate,
            &ProposedPlan {
                selected_candidates: vec![CandidateId(1), CandidateId(1)],
                assignments: vec![]
            }
        ),
        Err(EvaluationError::DuplicatePlacement(CandidateId(1)))
    );
    assert_eq!(
        evaluate(
            &one_candidate,
            &ProposedPlan {
                selected_candidates: vec![CandidateId(9)],
                assignments: vec![]
            }
        ),
        Err(EvaluationError::UnknownCandidate(CandidateId(9)))
    );
    assert_eq!(
        evaluate(
            &one_candidate,
            &ProposedPlan {
                selected_candidates: vec![],
                assignments: vec![DemandAssignment {
                    demand_id: DemandId(2),
                    candidate_id: CandidateId(1)
                }]
            }
        ),
        Err(EvaluationError::UnknownDemand(DemandId(2)))
    );
    assert_eq!(
        evaluate(
            &one_candidate,
            &ProposedPlan {
                selected_candidates: vec![],
                assignments: vec![DemandAssignment {
                    demand_id: DemandId(1),
                    candidate_id: CandidateId(9)
                }]
            }
        ),
        Err(EvaluationError::UnknownCandidate(CandidateId(9)))
    );

    let overflowing_area = problem(
        vec![demand(1, u64::MAX, 0, 0), demand(2, 1, 0, 0)],
        vec![],
        0,
        0,
    );
    assert_eq!(
        evaluate(&overflowing_area, &ProposedPlan::default()),
        Err(EvaluationError::ArithmeticOverflow("total area weight"))
    );

    let overflowing_cost = problem(
        vec![],
        vec![
            candidate(1, u64::MAX, 0, 0, vec![]),
            candidate(2, 1, 0, 0, vec![]),
        ],
        2,
        u64::MAX,
    );
    assert_eq!(
        evaluate(
            &overflowing_cost,
            &ProposedPlan {
                selected_candidates: vec![CandidateId(1), CandidateId(2)],
                assignments: vec![]
            }
        ),
        Err(EvaluationError::ArithmeticOverflow("installation cost"))
    );

    let overflowing_load = problem(
        vec![demand(1, 1, u64::MAX, 0), demand(2, 1, 1, 0)],
        vec![],
        0,
        0,
    );
    assert_eq!(
        evaluate(&overflowing_load, &ProposedPlan::default()),
        Err(EvaluationError::ArithmeticOverflow("total offered load"))
    );

    let over_limit = problem(
        vec![],
        (0..129).map(|id| candidate(id, 0, 0, 0, vec![])).collect(),
        0,
        0,
    );
    assert_eq!(
        evaluate(&over_limit, &ProposedPlan::default()),
        Err(EvaluationError::ResourceLimit("candidate count"))
    );
}

#[test]
fn public_measure_fields_have_stable_units() {
    let result = evaluate(
        &problem(
            vec![demand(1, 1, 1, 1)],
            vec![candidate(1, 1, 1, 1, vec![link(1, -600, -600)])],
            1,
            1,
        ),
        &ProposedPlan {
            selected_candidates: vec![CandidateId(1)],
            assignments: vec![DemandAssignment {
                demand_id: DemandId(1),
                candidate_id: CandidateId(1),
            }],
        },
    )
    .unwrap();
    assert_eq!(
        finding(
            &result,
            ConstraintKind::RadioCapacity,
            ConstraintScope::Candidate(CandidateId(1))
        )
        .actual,
        ConstraintMeasure::Throughput(kbps(1))
    );
}
