//! 已有建筑与批量候选共用的二维 BVH。叶只持原数组下标，不复制建筑描述或几何。
use super::*;

pub(in crate::authoring::buildings) trait BuildingBounds {
    fn bounds(&self) -> Rect;
    fn id(&self) -> &str;
}
impl BuildingBounds for IndexedBuilding {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn id(&self) -> &str {
        &self.id
    }
}

/// BVH 节点；叶节点引用 `order` 中一个连续小范围。
#[derive(Clone, Debug)]
enum BuildingBvhNode {
    Leaf {
        bounds: Rect, // 该叶全部建筑联合范围。
        start: usize, // `order` 起始下标。
        end: usize,   // `order` 排他末尾下标。
    },
    Branch {
        bounds: Rect, // 两个子树联合范围。
        left: usize,  // 左子节点下标。
        right: usize, // 右子节点下标。
    },
}

impl BuildingBvhNode {
    /// 返回节点世界 AABB。
    fn bounds(&self) -> Rect {
        match *self {
            Self::Leaf { bounds, .. } | Self::Branch { bounds, .. } => bounds,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(in crate::authoring::buildings) struct BuildingBoundsTree {
    order: Vec<usize>,
    nodes: Vec<BuildingBvhNode>,
}
impl BuildingBoundsTree {
    const LEAF_SIZE: usize = 8;
    pub(in crate::authoring::buildings) fn build(entries: &[impl BuildingBounds]) -> Self {
        let mut tree = Self {
            order: (0..entries.len()).collect(),
            nodes: Vec::new(),
        };
        if !entries.is_empty() {
            tree.build_node(entries, 0, entries.len());
        }
        tree
    }

    pub(in crate::authoring::buildings) fn query(
        &self,
        entries: &[impl BuildingBounds],
        bounds: Rect,
    ) -> Vec<usize> {
        if self.nodes.is_empty() || !finite_rect(bounds) {
            return Vec::new(); // 空索引或坏候选不会产生伪命中。
        }
        let mut result = Vec::new(); // 单次后台点击拥有自己的有界候选数组。
        let mut stack = vec![0_usize];
        while let Some(node_index) = stack.pop() {
            let node = &self.nodes[node_index];
            if !rects_overlap(node.bounds(), bounds) {
                continue; // 宽相位不相交时跳过完整子树。
            }
            match *node {
                BuildingBvhNode::Leaf { start, end, .. } => {
                    result.extend(
                        self.order[start..end]
                            .iter()
                            .copied()
                            .filter(|entry| rects_overlap(entries[*entry].bounds(), bounds)),
                    ); // 叶内继续用单项联合范围收紧候选。
                }
                BuildingBvhNode::Branch { left, right, .. } => {
                    stack.push(right); // 后进先出保证左树先处理，结果稳定。
                    stack.push(left);
                }
            }
        }
        result
    }

    fn build_node(&mut self, entries: &[impl BuildingBounds], start: usize, end: usize) -> usize {
        let bounds = self.bounds_for(entries, start, end); // 当前范围至少包含一个已校验项。
        let node_index = self.nodes.len();
        self.nodes.push(BuildingBvhNode::Leaf {
            bounds, // 递归期间使用合法占位节点。
            start,  // 占位范围与最终节点相同。
            end,    // 完成子树后可能替换为分支。
        });
        if end - start <= Self::LEAF_SIZE {
            return node_index; // 小范围直接成为叶节点。
        }
        let extent = bounds.max - bounds.min;
        let axis = usize::from(extent.y > extent.x); // 沿更长世界轴切分降低重叠。
        self.order[start..end].sort_by(|left, right| {
            let left_center = rect_center(entries[*left].bounds())[axis];
            let right_center = rect_center(entries[*right].bounds())[axis];
            left_center
                .total_cmp(&right_center)
                .then_with(|| entries[*left].id().cmp(entries[*right].id()))
        }); // 相同中心按稳定实例ID确定顺序。
        let middle = start + (end - start) / 2;
        let left = self.build_node(entries, start, middle);
        let right = self.build_node(entries, middle, end);
        self.nodes[node_index] = BuildingBvhNode::Branch {
            bounds, // 父范围已覆盖两个重新排列后的子范围。
            left,   // 左半连续范围节点。
            right,  // 右半连续范围节点。
        };
        node_index
    }

    /// 合并 `order` 当前连续范围的全部 AABB。
    fn bounds_for(&self, entries: &[impl BuildingBounds], start: usize, end: usize) -> Rect {
        let mut bounds = entries[self.order[start]].bounds();
        for entry in &self.order[start + 1..end] {
            bounds = union_rect(bounds, entries[*entry].bounds());
        }
        bounds
    }
}
