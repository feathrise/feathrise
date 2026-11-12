//! 四影响蒙皮数值约束。所有编辑工具共享锁定及归一化规则。

use feathrise_editor_glb_model::weights::VertexWeights;

const WIDTH: usize = 4;

pub(super) fn checked(
    joints: [u16; WIDTH],
    weights: [f32; WIDTH],
    count: usize,
) -> Option<VertexWeights> {
    let mut out = VertexWeights { joints, weights };
    for index in 0..WIDTH {
        let weight = weights[index];
        if !weight.is_finite()
            || weight < 0.0
            || joints[index] as usize >= count
            || weight > 0.0
                && (0..index).any(|old| weights[old] > 0.0 && joints[old] == joints[index])
        {
            return None;
        }
    }
    let sum: f32 = weights.iter().sum();
    if !sum.is_finite() || sum <= f32::EPSILON {
        return None;
    }
    for weight in &mut out.weights {
        *weight /= sum;
    }
    Some(out)
}

pub(super) fn edit(
    vertex: VertexWeights,
    joint: u16,
    requested: f32,
    locked: &[bool],
) -> VertexWeights {
    if !requested.is_finite()
        || locked.get(joint as usize) != Some(&false)
        || (requested - vertex.weight(joint)).abs() < 1e-6
    {
        return vertex;
    }
    let mut out = VertexWeights {
        joints: [0; WIDTH],
        weights: [0.0; WIDTH],
    };
    let mut keep = 0;
    let mut locked_sum = 0.0;
    let mut free = [(0, 0.0); WIDTH];
    let mut free_count = 0;
    for (index, weight) in vertex.joints.into_iter().zip(vertex.weights) {
        if weight <= 0.0 || index == joint {
            continue;
        }
        if locked.get(index as usize) == Some(&true) {
            out.joints[keep] = index;
            out.weights[keep] = weight;
            keep += 1;
            locked_sum += weight;
        } else {
            free[free_count] = (index, weight);
            free_count += 1;
        }
    }

    let available = (1.0_f32 - locked_sum).max(0.0);
    if keep == WIDTH || available <= f32::EPSILON {
        return vertex;
    }
    let mut weight = requested.clamp(0.0, available);
    free[..free_count].sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let rest_count = free_count.min(WIDTH - keep - usize::from(weight > 0.0));
    let rest = &free[..rest_count];
    let sum: f32 = rest.iter().map(|entry| entry.1).sum();
    if sum == 0.0 {
        // 没有可承接的骨骼时保留原权重。指定新归属可转走这份权重。
        if weight == 0.0 {
            return vertex;
        }
        weight = available;
    }
    if weight > 0.0 {
        out.joints[keep] = joint;
        out.weights[keep] = weight;
        keep += 1;
    }
    let remaining = available - weight;
    if remaining > 0.0 {
        for &(index, prior) in rest {
            out.joints[keep] = index;
            out.weights[keep] = remaining * (prior / sum);
            keep += 1;
        }
    }
    out
}

pub(super) fn opposite_name(name: &str) -> Option<String> {
    let name = name.to_lowercase();
    let (namespace, tail) = name.rsplit_once(':').unwrap_or(("", &name));
    let alternate = if let Some(part) = tail.strip_prefix("left") {
        format!("right{part}")
    } else if let Some(part) = tail.strip_prefix("right") {
        format!("left{part}")
    } else {
        [
            (".l", ".r"),
            ("_l", "_r"),
            ("-l", "-r"),
            ("_left", "_right"),
            (".left", ".right"),
        ]
        .into_iter()
        .find_map(|(left, right)| {
            tail.strip_suffix(left)
                .map(|part| format!("{part}{right}"))
                .or_else(|| tail.strip_suffix(right).map(|part| format!("{part}{left}")))
        })?
    };
    Some(if namespace.is_empty() {
        alternate
    } else {
        format!("{namespace}:{alternate}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_weights_survive_added_influence_and_normalization() {
        let vertex = checked([1, 2, 3, 4], [0.1, 0.2, 0.3, 0.4], 6).unwrap();
        let mut locks = [false; 6];
        locks[1] = true;
        let changed = edit(vertex, 5, 0.5, &locks);
        assert_eq!(changed.weight(1), vertex.weight(1));
        assert_eq!(changed.weight(5), 0.5);
        assert_eq!(changed.weight(2), 0.0);
        assert!((changed.weights.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert_eq!(edit(changed, 1, 0.0, &locks), changed);
    }

    #[test]
    fn sole_influence_requires_a_replacement() {
        let vertex = checked([1, 0, 0, 0], [1.0, 0.0, 0.0, 0.0], 3).unwrap();
        assert_eq!(edit(vertex, 1, 0.0, &[false; 3]), vertex);
        assert_eq!(edit(vertex, 2, 1.0, &[false; 3]).weight(2), 1.0);
        assert_eq!(
            opposite_name("mixamorig:LeftForeArm").as_deref(),
            Some("mixamorig:rightforearm")
        );
        assert_eq!(opposite_name("thumb.R").as_deref(), Some("thumb.l"));
    }
}
