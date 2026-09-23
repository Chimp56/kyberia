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
fn rssi_thresholds_are_inclusive_at_each_tenth_dbm_boundary() {
    let cases = [
        (-650, -680, None),
        (-651, -680, Some(SignalFailure::Downlink)),
        (-649, -680, None),
        (-650, -681, Some(SignalFailure::Uplink)),
        (-650, -679, None),
    ];

    for (downlink, uplink, failure) in cases {
        let problem = problem(
            vec![demand(1, 1, 0, 0)],
            vec![candidate(1, 0, 0, 0, vec![link(1, downlink, uplink)])],
            1,
            0,
        );
        let plan = ProposedPlan {
            selected_candidates: vec![CandidateId(1)],
            assignments: vec![DemandAssignment {
                demand_id: DemandId(1),
                candidate_id: CandidateId(1),
            }],
        };
        let result = evaluate(&problem, &plan).unwrap();
        let coverage = finding(
            &result,
            ConstraintKind::BidirectionalCoverage,
            ConstraintScope::Demand(DemandId(1)),
        );
        let assignment = finding(
            &result,
            ConstraintKind::AssignedDemandLink,
            ConstraintScope::Demand(DemandId(1)),
        );

        match failure {
            None => {
                assert_eq!(coverage.status, ConstraintStatus::Binding);
                assert_eq!(assignment.status, ConstraintStatus::Binding);
                assert_eq!(result.objectives.passing_area_weight, AreaWeight(1));
                assert_eq!(result.objectives.nonpassing_area_weight, AreaWeight(0));
                match &coverage.evidence {
                    ConstraintEvidence::Coverage {
                        eligible_selected,
                        below_threshold_selected,
                        ..
                    } => {
                        assert_eq!(eligible_selected, &vec![CandidateId(1)]);
                        assert!(below_threshold_selected.is_empty());
                    }
                    evidence => panic!("unexpected evidence: {evidence:?}"),
                }
            }
            Some(expected_failure) => {
                assert_eq!(coverage.status, ConstraintStatus::Violated);
                assert_eq!(assignment.status, ConstraintStatus::Violated);
                assert_eq!(result.objectives.passing_area_weight, AreaWeight(0));
                assert_eq!(result.objectives.nonpassing_area_weight, AreaWeight(1));
                match &coverage.evidence {
                    ConstraintEvidence::Coverage {
                        eligible_selected,
                        below_threshold_selected,
                        ..
                    } => {
                        assert!(eligible_selected.is_empty());
                        assert_eq!(below_threshold_selected.len(), 1);
                        assert_eq!(below_threshold_selected[0].failure, expected_failure);
                    }
                    evidence => panic!("unexpected evidence: {evidence:?}"),
                }
            }
        }
    }
}

#[test]
fn represented_hard_constraints_classify_values_adjacent_to_their_limits() {
    let expected = [
        ConstraintStatus::Satisfied,
        ConstraintStatus::Binding,
        ConstraintStatus::Violated,
    ];

    for ((selected_count, limit), expected_status) in
        [(1, 2), (1, 1), (2, 1)].into_iter().zip(expected)
    {
        let problem = problem(
            vec![],
            (0..2).map(|id| candidate(id, 0, 1, 1, vec![])).collect(),
            limit,
            0,
        );
        let plan = ProposedPlan {
            selected_candidates: (0..selected_count).map(CandidateId).collect(),
            assignments: vec![],
        };
        let result = evaluate(&problem, &plan).unwrap();
        assert_eq!(
            finding(
                &result,
                ConstraintKind::AccessPointCount,
                ConstraintScope::Plan
            )
            .status,
            expected_status
        );
        assert_eq!(
            result.feasible,
            expected_status != ConstraintStatus::Violated
        );
    }

    for ((cost, budget), expected_status) in [(9, 10), (10, 10), (11, 10)].into_iter().zip(expected)
    {
        let problem = problem(vec![], vec![candidate(1, cost, 1, 1, vec![])], 1, budget);
        let plan = ProposedPlan {
            selected_candidates: vec![CandidateId(1)],
            assignments: vec![],
        };
        let result = evaluate(&problem, &plan).unwrap();
        assert_eq!(
            finding(
                &result,
                ConstraintKind::InstallationBudget,
                ConstraintScope::Plan
            )
            .status,
            expected_status
        );
        assert_eq!(
            result.feasible,
            expected_status != ConstraintStatus::Violated
        );
    }

    for ((offered_load, capacity), expected_status) in
        [(9, 10), (10, 10), (11, 10)].into_iter().zip(expected)
    {
        let problem = problem(
            vec![demand(1, 1, offered_load, 0)],
            vec![candidate(1, 0, capacity, 1, vec![link(1, -600, -600)])],
            1,
            0,
        );
        let plan = ProposedPlan {
            selected_candidates: vec![CandidateId(1)],
            assignments: vec![DemandAssignment {
                demand_id: DemandId(1),
                candidate_id: CandidateId(1),
            }],
        };
        let result = evaluate(&problem, &plan).unwrap();
        assert_eq!(
            finding(
                &result,
                ConstraintKind::RadioCapacity,
                ConstraintScope::Candidate(CandidateId(1))
            )
            .status,
            expected_status
        );
        assert_eq!(
            result.feasible,
            expected_status != ConstraintStatus::Violated
        );
    }

    for ((client_count, client_limit), expected_status) in
        [(9, 10), (10, 10), (11, 10)].into_iter().zip(expected)
    {
        let mut demand = demand(1, 1, 0, client_count);
        demand.minimum_downlink = dbm(-650);
        demand.minimum_uplink = dbm(-680);
        let problem = problem(
            vec![demand],
            vec![candidate(1, 0, 1, client_limit, vec![link(1, -600, -600)])],
            1,
            0,
        );
        let plan = ProposedPlan {
            selected_candidates: vec![CandidateId(1)],
            assignments: vec![DemandAssignment {
                demand_id: DemandId(1),
                candidate_id: CandidateId(1),
            }],
        };
        let result = evaluate(&problem, &plan).unwrap();
        assert_eq!(
            finding(
                &result,
                ConstraintKind::RadioClientCount,
                ConstraintScope::Candidate(CandidateId(1))
            )
            .status,
            expected_status
        );
        assert_eq!(
            result.feasible,
            expected_status != ConstraintStatus::Violated
        );
    }

    for (eligible_count, expected_status) in [
        (1, ConstraintStatus::Violated),
        (2, ConstraintStatus::Binding),
        (3, ConstraintStatus::Satisfied),
    ] {
        let mut demand = demand(1, 1, 0, 0);
        demand.required_distinct_access_points = 2;
        let problem = problem(
            vec![demand],
            (0..3)
                .map(|id| {
                    let estimate = if id < eligible_count {
                        link(1, -600, -600)
                    } else {
                        link(1, -651, -680)
                    };
                    candidate(id, 0, 1, 1, vec![estimate])
                })
                .collect(),
            3,
            0,
        );
        let plan = ProposedPlan {
            selected_candidates: (0..3).map(CandidateId).collect(),
            assignments: vec![DemandAssignment {
                demand_id: DemandId(1),
                candidate_id: CandidateId(0),
            }],
        };
        let result = evaluate(&problem, &plan).unwrap();
        assert_eq!(
            finding(
                &result,
                ConstraintKind::BidirectionalCoverage,
                ConstraintScope::Demand(DemandId(1))
            )
            .status,
            expected_status
        );
        assert_eq!(
            result.feasible,
            expected_status != ConstraintStatus::Violated
        );
    }
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
fn missing_selected_link_does_not_make_sufficient_known_coverage_unknown() {
    let problem = problem(
        vec![demand(1, 5, 0, 0)],
        vec![
            candidate(1, 0, 0, 0, vec![link(1, -650, -680)]),
            candidate(2, 0, 0, 0, vec![]),
        ],
        2,
        0,
    );
    let plan = ProposedPlan {
        selected_candidates: vec![CandidateId(1), CandidateId(2)],
        assignments: vec![DemandAssignment {
            demand_id: DemandId(1),
            candidate_id: CandidateId(1),
        }],
    };

    let result = evaluate(&problem, &plan).unwrap();

    assert!(result.feasible);
    assert_eq!(result.objectives.passing_area_weight, AreaWeight(5));
    assert_eq!(result.objectives.nonpassing_area_weight, AreaWeight(0));
    assert_eq!(result.objectives.unverified_area_weight, AreaWeight(0));
    let coverage = finding(
        &result,
        ConstraintKind::BidirectionalCoverage,
        ConstraintScope::Demand(DemandId(1)),
    );
    assert_eq!(coverage.status, ConstraintStatus::Binding);
    assert!(matches!(
        &coverage.evidence,
        ConstraintEvidence::Coverage {
            eligible_selected,
            unknown_selected,
            ..
        } if eligible_selected == &vec![CandidateId(1)]
            && unknown_selected == &vec![CandidateId(2)]
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

#[test]
fn resource_limits_accept_the_ceiling_and_reject_ceiling_plus_one() {
    // These contract ceilings mirror the private evaluator guards. Keep the
    // first admitted and rejected sizes adjacent to catch off-by-one changes.
    const MAX_CANDIDATES: u32 = 128;
    const MAX_DEMANDS: u32 = 4_096;
    const MAX_LINK_ESTIMATES: u32 = 131_072;

    let max_candidates = problem(
        vec![],
        (0..MAX_CANDIDATES)
            .map(|id| candidate(id, 0, 0, 0, vec![]))
            .collect(),
        MAX_CANDIDATES as u16,
        0,
    );
    let all_candidates_selected = ProposedPlan {
        selected_candidates: (0..MAX_CANDIDATES).map(CandidateId).collect(),
        assignments: vec![],
    };
    assert!(
        evaluate(&max_candidates, &all_candidates_selected)
            .unwrap()
            .feasible
    );

    let too_many_candidates = problem(
        vec![],
        (0..=MAX_CANDIDATES)
            .map(|id| candidate(id, 0, 0, 0, vec![]))
            .collect(),
        0,
        0,
    );
    assert_eq!(
        evaluate(&too_many_candidates, &ProposedPlan::default()),
        Err(EvaluationError::ResourceLimit("candidate count"))
    );

    let too_many_selected = ProposedPlan {
        selected_candidates: (0..=MAX_CANDIDATES)
            .map(|id| CandidateId(id % MAX_CANDIDATES))
            .collect(),
        assignments: vec![],
    };
    assert_eq!(
        evaluate(&max_candidates, &too_many_selected),
        Err(EvaluationError::ResourceLimit("selected candidate count"))
    );

    let max_demands = problem(
        (0..MAX_DEMANDS).map(|id| demand(id, 1, 0, 0)).collect(),
        vec![],
        0,
        0,
    );
    assert!(evaluate(&max_demands, &ProposedPlan::default()).is_ok());
    let too_many_demands = problem(
        (0..=MAX_DEMANDS).map(|id| demand(id, 1, 0, 0)).collect(),
        vec![],
        0,
        0,
    );
    assert_eq!(
        evaluate(&too_many_demands, &ProposedPlan::default()),
        Err(EvaluationError::ResourceLimit("demand count"))
    );

    let demand_ids: Vec<_> = (0..MAX_DEMANDS).map(DemandId).collect();
    let assignment_limit_problem = problem(
        demand_ids.iter().map(|id| demand(id.0, 1, 0, 0)).collect(),
        vec![candidate(
            1,
            0,
            0,
            0,
            demand_ids.iter().map(|id| link(id.0, -600, -600)).collect(),
        )],
        1,
        0,
    );
    let max_assignments = ProposedPlan {
        selected_candidates: vec![CandidateId(1)],
        assignments: demand_ids
            .iter()
            .map(|demand_id| DemandAssignment {
                demand_id: *demand_id,
                candidate_id: CandidateId(1),
            })
            .collect(),
    };
    assert!(
        evaluate(&assignment_limit_problem, &max_assignments)
            .unwrap()
            .feasible
    );
    let too_many_assignments = ProposedPlan {
        selected_candidates: vec![],
        assignments: (0..=MAX_DEMANDS)
            .map(|id| DemandAssignment {
                demand_id: DemandId(id),
                candidate_id: CandidateId(0),
            })
            .collect(),
    };
    assert_eq!(
        evaluate(&problem(vec![], vec![], 0, 0), &too_many_assignments),
        Err(EvaluationError::ResourceLimit("assignment count"))
    );

    let make_link_limited_problem = |demand_count: u32, one_over: bool| {
        problem(
            (0..demand_count).map(|id| demand(id, 1, 0, 0)).collect(),
            (0..MAX_CANDIDATES)
                .map(|candidate_id| {
                    let mut links: Vec<_> = (0..1_024)
                        .map(|demand_id| link(demand_id, -600, -600))
                        .collect();
                    if candidate_id == 0 && one_over {
                        links.push(link(1_024, -600, -600));
                    }
                    candidate(candidate_id, 0, 0, 0, links)
                })
                .collect(),
            0,
            0,
        )
    };
    let max_links = make_link_limited_problem(1_024, false);
    assert_eq!(
        max_links
            .candidates
            .iter()
            .map(|candidate| candidate.links.len() as u32)
            .sum::<u32>(),
        MAX_LINK_ESTIMATES
    );
    assert!(evaluate(&max_links, &ProposedPlan::default()).is_ok());
    let too_many_links = make_link_limited_problem(1_025, true);
    assert_eq!(
        evaluate(&too_many_links, &ProposedPlan::default()),
        Err(EvaluationError::ResourceLimit("link estimate count"))
    );
}
