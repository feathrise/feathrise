//! 建筑分页批次的跨页、缺页、换源、故障与硬预算回归。
use super::*;
use std::collections::BTreeMap;

fn batch(points: Vec<Vec2>) -> BuildingGroundQuery {
    BuildingGroundQuery {
        content: Some([7; 7]),
        points,
        ..default()
    }
}

fn page(revision: u64) -> Option<(Option<u64>, bool)> {
    Some((Some(revision), true))
}

#[test]
fn missing_corner_keeps_exact_prefix_and_never_reports_a_complete_foundation() {
    let mut query = batch(vec![Vec2::ZERO, Vec2::ONE, Vec2::new(2., 2.)]);
    query
        .advance(Some([7; 7]), page(1), |point| {
            Ok(if point == Vec2::ZERO {
                PlacementTerrainSample::Height(5.)
            } else {
                PlacementTerrainSample::WaitingForHeight
            })
        })
        .unwrap();
    assert_eq!(query.cursor, 1);
    assert_eq!(query.heights.len(), 1);
    assert!(!query.can_retry_for(Some([7; 7]), page(1)).unwrap());
    assert!(query.can_retry_for(Some([7; 7]), page(2)).unwrap());
    query
        .advance(Some([7; 7]), page(2), |point| {
            assert_ne!(point, Vec2::ZERO); // 已完成点即使所属页被淘汰也不重采或换成粗显示高度。
            Ok(PlacementTerrainSample::Height(10. + point.x))
        })
        .unwrap();
    assert_eq!(query.cursor, query.points.len());
    assert_eq!(query.heights[&(0f32.to_bits(), 0f32.to_bits())], 5.);
}

#[test]
fn more_than_sixty_four_pages_complete_without_requiring_simultaneous_residency() {
    let mut query = batch(
        (0..70)
            .map(|index| Vec2::new(index as f32 * 256., 1.))
            .collect(),
    );
    for loaded in 0..70 {
        query
            .advance(Some([7; 7]), page(loaded as u64), |point| {
                let page = (point.x / 256.) as usize;
                assert!(page >= loaded); // 此夹具只有当前一页可读，已淘汰页不再提供高度。
                Ok(if page == loaded {
                    PlacementTerrainSample::Height(page as f32 + 3.)
                } else {
                    PlacementTerrainSample::WaitingForHeight
                })
            })
            .unwrap();
        assert_eq!(query.cursor, loaded + 1);
    }
    let sampler = PlacementTerrainSampler::from_clipmap_samples([7; 7], query.heights);
    assert_eq!(sampler.sample(0., 1.).unwrap(), 3.);
    assert_eq!(sampler.sample(69. * 256., 1.).unwrap(), 72.);
}

#[test]
fn source_change_rejects_the_whole_prefix_before_reading_new_samples() {
    let mut query = batch(vec![Vec2::ZERO, Vec2::ONE]);
    query
        .advance(Some([7; 7]), page(1), |point| {
            Ok(if point == Vec2::ZERO {
                PlacementTerrainSample::Height(4.)
            } else {
                PlacementTerrainSample::WaitingForHeight
            })
        })
        .unwrap();
    assert!(
        query
            .advance(Some([8; 7]), page(2), |_| panic!("旧批次不得读取新内容"))
            .is_err()
    );
    assert!(query.can_retry_for(Some([8; 7]), page(2)).is_err());
    assert_eq!(query.cursor, 1);
    assert_eq!(query.heights.len(), 1);
}

#[test]
fn corrupt_pages_and_nonfinite_heights_are_failures_instead_of_zero_ground() {
    for corrupt in [true, false] {
        let mut query = batch(vec![Vec2::ONE]);
        let result = query.advance(Some([7; 7]), page(1), |_| {
            if corrupt {
                Err("损坏页".into())
            } else {
                Ok(PlacementTerrainSample::Height(f32::NAN))
            }
        });
        assert!(result.is_err());
        assert!(query.heights.is_empty());
        assert_eq!(query.cursor, 0);
    }
}

#[test]
fn exact_duplicates_are_sampled_once_and_batch_is_hard_bounded() {
    let sampler = PlacementTerrainSampler::from_clipmap_samples([7; 7], BTreeMap::new());
    let mut query = BuildingGroundQuery::new(&sampler, [Vec2::ONE; 5]);
    let mut calls = 0;
    query
        .advance(Some([7; 7]), page(1), |_| {
            calls += 1;
            Ok(PlacementTerrainSample::Height(6.))
        })
        .unwrap();
    assert_eq!(calls, 1);
    let mut oversized = BuildingGroundQuery::new(&sampler, std::iter::repeat(Vec2::ONE));
    assert_eq!(
        oversized.points.len(),
        5 * feathrise_scene_content::MAX_SCENE_PLACEMENTS_PER_SCENE + 1
    );
    assert!(oversized.prepare(&sampler).is_err());
}

#[test]
fn page_binding_and_adoption_gate_can_resume_without_changing_the_page_number() {
    let mut query = batch(vec![Vec2::ONE]);
    query
        .advance(Some([7; 7]), Some((None, true)), |_| {
            Ok(PlacementTerrainSample::WaitingForHeight)
        })
        .unwrap();
    assert!(query.can_retry_for(Some([7; 7]), page(0)).unwrap());
    query.waiting_page = Some((Some(0), false));
    assert!(query.can_retry_for(Some([7; 7]), page(0)).unwrap());
    assert!(
        !query
            .can_retry_for(Some([7; 7]), Some((Some(0), false)))
            .unwrap()
    );
}
