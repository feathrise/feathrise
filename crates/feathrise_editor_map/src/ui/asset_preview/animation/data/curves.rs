//! 曲线采样与关键帧结构操作；插入、删除和重排始终以完整关键帧携带三次切线。

use super::types::*;
use bevy::math::Quat;

impl Channel {
    pub fn dimensions(&self) -> usize {
        if self.path == "rotation" { 4 } else { 3 }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.path.as_str(), "translation" | "rotation" | "scale") {
            return Err("只能编辑位置、旋转或缩放通道".into());
        }
        if !matches!(
            self.interpolation.as_str(),
            "STEP" | "LINEAR" | "CUBICSPLINE"
        ) {
            return Err("插值必须为 STEP、LINEAR 或 CUBICSPLINE".into());
        }
        if self.keys.is_empty() {
            return Err("动画通道至少需要一个关键帧".into());
        }
        let dimensions = self.dimensions();
        let mut previous = None;
        for key in &self.keys {
            if !key.time.is_finite()
                || key.time < 0.0
                || previous.is_some_and(|time| key.time <= time)
            {
                return Err("关键帧时间须为非负有限数且严格递增，不能存在同一时刻的重复帧".into());
            }
            previous = Some(key.time);
            check_components(&key.value, dimensions)?;
            if self.path == "rotation" {
                check_rotation(&key.value)?;
            }
            if self.interpolation == "CUBICSPLINE" {
                check_components(&key.in_tangent, dimensions)?;
                check_components(&key.out_tangent, dimensions)?;
            }
        }
        Ok(())
    }

    /// 返回可直接用于节点局部 TRS 的值；四元数保持 glTF 的 XYZW 分量顺序。
    pub fn sample(&self, time: f32) -> Vec<f32> {
        let Some(first) = self.keys.first() else {
            return Vec::new();
        };
        if self.keys.len() == 1 || !time.is_finite() || time <= first.time {
            return self.normalized_rotation(first.value.clone());
        }
        let upper = self.keys.partition_point(|key| key.time <= time);
        if upper == self.keys.len() {
            return self.normalized_rotation(self.keys[upper - 1].value.clone());
        }
        let before = &self.keys[upper - 1];
        let after = &self.keys[upper];
        if self.interpolation == "STEP" {
            return self.normalized_rotation(before.value.clone());
        }
        let duration = after.time - before.time;
        let amount = (time - before.time) / duration;
        if self.interpolation == "LINEAR"
            && self.path == "rotation"
            && before.value.len() == 4
            && after.value.len() == 4
        {
            let before = quat(&before.value);
            let after = quat(&after.value);
            return before.slerp(after, amount).normalize().to_array().to_vec();
        }
        let values = if self.interpolation == "CUBICSPLINE" {
            hermite(before, after, amount, duration, false)
        } else {
            before
                .value
                .iter()
                .zip(&after.value)
                .map(|(before, after)| {
                    (*before as f64 + (*after as f64 - *before as f64) * amount as f64) as f32
                })
                .collect()
        };
        // 极端作者值的 Hermite 过冲可能超出 f32；预览保持上一有效姿态，不能发布 NaN/Inf。
        if values.iter().any(|value| !value.is_finite()) {
            return self.normalized_rotation(before.value.clone());
        }
        self.normalized_rotation(values)
    }

    /// 插入时初始化每秒导数；同一时刻改值会沿用原切线，不产生重复时间。
    pub fn insert_key(&mut self, time: f32, value: Vec<f32>) -> Result<usize, String> {
        if !time.is_finite() || time < 0.0 {
            return Err("关键帧时间必须为非负有限数".into());
        }
        check_components(&value, self.dimensions())?;
        if self.path == "rotation" {
            check_rotation(&value)?;
        }
        let value = self.normalized_rotation(value);
        let index = self.keys.partition_point(|key| key.time < time);
        if self.keys.get(index).is_some_and(|key| key.time == time) {
            self.keys[index].value = value;
            return Ok(index);
        }
        let tangent = if self.interpolation == "CUBICSPLINE" {
            if index > 0 && index < self.keys.len() {
                let before = &self.keys[index - 1];
                let after = &self.keys[index];
                let duration = after.time - before.time;
                // Hermite 细分沿用相邻切线，新帧取原曲线导数；插入采样值时不破坏平移/缩放轨迹。
                hermite(
                    before,
                    after,
                    (time - before.time) / duration,
                    duration,
                    true,
                )
            } else {
                vec![0.0; self.dimensions()]
            }
        } else {
            Vec::new()
        };
        if self.interpolation == "CUBICSPLINE" {
            check_components(&tangent, self.dimensions())?;
        }
        self.keys.insert(
            index,
            Keyframe {
                time,
                value,
                in_tangent: tangent.clone(),
                out_tangent: tangent,
            },
        );
        Ok(index)
    }

    pub fn delete_key(&mut self, index: usize) -> Result<(), String> {
        if index >= self.keys.len() {
            return Err("关键帧已不存在".into());
        }
        if self.keys.len() == 1 {
            return Err("每条通道至少保留一个关键帧".into());
        }
        self.keys.remove(index);
        Ok(())
    }

    pub fn move_key(&mut self, index: usize, time: f32) -> Result<usize, String> {
        if index >= self.keys.len() {
            return Err("关键帧已不存在".into());
        }
        if !time.is_finite() || time < 0.0 {
            return Err("关键帧时间必须为非负有限数".into());
        }
        if self
            .keys
            .iter()
            .enumerate()
            .any(|(other, key)| other != index && key.time == time)
        {
            return Err("此时刻已有关键帧，请选择其他时间".into());
        }
        let mut key = self.keys.remove(index);
        key.time = time;
        let next = self.keys.partition_point(|key| key.time < time);
        self.keys.insert(next, key);
        Ok(next)
    }

    /// 从普通曲线切为三次样条时显式补零切线，避免保存不完整输出；切回时保留切线便于撤销。
    pub fn set_interpolation(&mut self, interpolation: &str) -> Result<(), String> {
        if !matches!(interpolation, "STEP" | "LINEAR" | "CUBICSPLINE") {
            return Err("不支持的插值模式".into());
        }
        if interpolation == "CUBICSPLINE" {
            let dimensions = self.dimensions();
            for key in &mut self.keys {
                if key.in_tangent.len() != dimensions {
                    key.in_tangent = vec![0.0; dimensions];
                }
                if key.out_tangent.len() != dimensions {
                    key.out_tangent = vec![0.0; dimensions];
                }
            }
        }
        self.interpolation = interpolation.into();
        Ok(())
    }

    fn normalized_rotation(&self, mut values: Vec<f32>) -> Vec<f32> {
        if self.path == "rotation" && values.len() == 4 {
            let rotation = quat(&values);
            values.copy_from_slice(&rotation.to_array());
        }
        values
    }
}

fn quat(values: &[f32]) -> Quat {
    let rotation = Quat::from_xyzw(values[0], values[1], values[2], values[3]);
    if rotation.is_finite()
        && rotation.length_squared().is_finite()
        && rotation.length_squared() > 1.0e-12
    {
        rotation.normalize()
    } else {
        Quat::IDENTITY
    }
}

fn check_components(values: &[f32], dimensions: usize) -> Result<(), String> {
    if values.len() != dimensions || values.iter().any(|value| !value.is_finite()) {
        return Err(format!("通道值须为 {dimensions} 个有限浮点数"));
    }
    Ok(())
}

fn check_rotation(values: &[f32]) -> Result<(), String> {
    let length = values.iter().map(|value| value * value).sum::<f32>();
    if !length.is_finite() || length < 1.0e-12 {
        return Err("旋转四元数必须为可归一化的非零值".into());
    }
    Ok(())
}

fn hermite(
    before: &Keyframe,
    after: &Keyframe,
    amount: f32,
    duration: f32,
    derivative: bool,
) -> Vec<f32> {
    let t = amount as f64;
    let duration = duration as f64;
    let (h00, h10, h01, h11) = if derivative {
        (
            (6.0 * t * t - 6.0 * t) / duration,
            3.0 * t * t - 4.0 * t + 1.0,
            (-6.0 * t * t + 6.0 * t) / duration,
            3.0 * t * t - 2.0 * t,
        )
    } else {
        (
            2.0 * t * t * t - 3.0 * t * t + 1.0,
            (t * t * t - 2.0 * t * t + t) * duration,
            -2.0 * t * t * t + 3.0 * t * t,
            (t * t * t - t * t) * duration,
        )
    };
    before
        .value
        .iter()
        .zip(&after.value)
        .enumerate()
        .map(|(index, (before_value, after_value))| {
            (*before_value as f64 * h00
                + before.out_tangent.get(index).copied().unwrap_or(0.0) as f64 * h10
                + *after_value as f64 * h01
                + after.in_tangent.get(index).copied().unwrap_or(0.0) as f64 * h11)
                as f32
        })
        .collect()
}
