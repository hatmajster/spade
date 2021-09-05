use std::convert::TryInto;

use crate::{handles::FixedVertexHandle, HasPosition, Point2, SpadeNum, Triangulation};

#[derive(Debug, PartialEq, PartialOrd, Clone, Copy)]
struct FloatOrd(f64);

impl Eq for FloatOrd {}

impl Ord for FloatOrd {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

pub(crate) fn bulk_load<V, T>(mut elements: Vec<V>) -> T
where
    V: HasPosition,
    T: Triangulation<Vertex = V>,
{
    if elements.is_empty() {
        return T::new();
    }

    let mut min = elements[0].position();
    let mut max = elements[0].position();

    for element in &elements {
        let position = element.position();
        min = min.min(position);
        max = max.max(position);
    }

    let center = min.add(max).mul(0.5f32.into());

    // Sort by distance, smallest values last. This allows to pop values depending on their distance.
    elements.sort_unstable_by(|a, b| {
        center
            .distance2(b.position())
            .partial_cmp(&center.distance2(a.position()))
            .unwrap()
    });

    let mut result = T::new();
    while let Some(next) = elements.pop() {
        result.insert(next);
        if !result.all_vertices_on_line() {
            break;
        }
    }

    let mut angles = Hull::from_triangulation(&result, center);

    while let Some(next) = elements.pop() {
        let next_position = next.position();
        let current_angle = pseudo_angle(next_position, center);
        let hint = angles.get(current_angle);
        let handle = result.insert_with_hint(next, hint);

        let vertex = result.vertex(handle);

        let outgoing_ch_edge = vertex.out_edges().find(|edge| edge.is_outer_edge());
        if let Some(second_edge) = outgoing_ch_edge {
            let first_edge = second_edge.prev();

            let first_angle = pseudo_angle(first_edge.from().position(), center);
            let second_angle = pseudo_angle(second_edge.to().position(), center);

            angles.insert(
                first_angle,
                current_angle,
                second_angle,
                first_edge.from().fix(),
            );
        } else {
            // Vertex is not part of the convex hull and was inserted into the inside of the triangulation.
            // Nothing to update since the convex  hull stayed the same
            continue;
        }
    }

    result
}

type Index = u32;

////////////////////////////////////////////////////////////////////////////////

/// This represents a strongly-typed index into a [`TypedVec`] parameterized
/// with the same `PhantomData`.  It should be zero-cost at runtime.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd)]
pub struct TypedIndex<P>(pub Index, std::marker::PhantomData<*const P>);
impl<P> TypedIndex<P> {
    pub fn new(i: usize) -> Self {
        Self::const_new(i.try_into().unwrap())
    }

    pub const fn const_new(i: Index) -> Self {
        Self(i, std::marker::PhantomData)
    }
}

impl<P> std::ops::Add<usize> for TypedIndex<P> {
    type Output = Self;
    fn add(self, i: usize) -> Self::Output {
        Self::new((self.0 as usize).checked_add(i).unwrap())
    }
}

impl<P> std::ops::AddAssign<usize> for TypedIndex<P> {
    fn add_assign(&mut self, i: usize) {
        let i: Index = i.try_into().unwrap();
        self.0 = self.0.checked_add(i).unwrap();
    }
}

impl<P> std::cmp::PartialEq<usize> for TypedIndex<P> {
    fn eq(&self, i: &usize) -> bool {
        (self.0 as usize).eq(i)
    }
}

////////////////////////////////////////////////////////////////////////////////

/// This represents a strongly-typed `Vec<T>` which can only be accessed by
/// a [`TypedIndex`] parameterized with the same `PhantomData`, at zero
/// run-time cost.
#[derive(Debug)]
pub struct TypedVec<T, P>(Vec<T>, std::marker::PhantomData<*const P>);

impl<T, P> std::ops::Index<TypedIndex<P>> for TypedVec<T, P> {
    type Output = T;
    fn index(&self, index: TypedIndex<P>) -> &Self::Output {
        self.0.index(index.0 as usize)
    }
}

impl<T, P> std::ops::IndexMut<TypedIndex<P>> for TypedVec<T, P> {
    fn index_mut(&mut self, index: TypedIndex<P>) -> &mut Self::Output {
        self.0.index_mut(index.0 as usize)
    }
}

impl<T, P> std::ops::Deref for TypedVec<T, P> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

impl<T, P> std::ops::DerefMut for TypedVec<T, P> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.0
    }
}

impl<T, P> TypedVec<T, P> {
    pub fn with_capacity(s: usize) -> Self {
        Self::of(Vec::with_capacity(s))
    }
    pub fn of(v: Vec<T>) -> Self {
        Self(v, std::marker::PhantomData)
    }
    pub fn push(&mut self, t: T) -> TypedIndex<P> {
        let i = self.next_index();
        self.0.push(t);
        i
    }
    pub fn next_index(&self) -> TypedIndex<P> {
        TypedIndex::new(self.0.len())
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd)]
pub struct HullTag {}
pub type HullIndex = TypedIndex<HullTag>;
pub type HullVec<T> = TypedVec<T, HullTag>;

#[derive(Clone, Copy, Debug)]
struct Segment {
    from: FloatOrd,
    to: FloatOrd,
}

impl Segment {
    fn new(from: FloatOrd, to: FloatOrd) -> Self {
        assert_ne!(from, to);
        Self { from, to }
    }

    fn is_non_wrapping_segment(&self) -> bool {
        self.from < self.to
    }

    fn contains_angle(&self, angle: FloatOrd) -> bool {
        if self.is_non_wrapping_segment() {
            self.from <= angle && angle < self.to
        } else {
            self.from <= angle || angle < self.to
        }
    }

    pub(crate) fn is_greater_than_180_degree(&self) -> bool {
        if self.is_non_wrapping_segment() {
            self.to.0 - self.from.0 > 0.5
        } else {
            self.to.0 + 1.0 - self.from.0 > 0.5
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Node {
    /// Pseudo-angle of the point
    angle: FloatOrd,

    /// TODO: Adjust comment
    /// `EdgeIndex` of the edge to the right of this point, i.e. having this
    /// point as its `dst` (since the hull is on top of the shape and triangle
    /// are wound counter-clockwise).
    vertex: FixedVertexHandle,

    /// Neighbors, or `EMPTY_HULL`
    left: HullIndex,
    right: HullIndex,
}

/// The Hull stores a set of points which form a left-to-right order
///
/// Each point is associated with an EdgeIndex into a half-edge data structure,
/// but the Hull does not concern itself with such things.
///
/// The Hull supports one kind of lookup: for a point P, find the point Q with
/// the highest X value that is below P.  When projecting P towards the
/// sweepline, it will intersect the edge beginning at Q; this edge is the one
/// which should be split.
///
/// In addition, the Hull stores a random-access map from PointIndex to
/// HullIndex (if present), for fast lookups without hash traversal.
#[derive(Debug)]
pub struct Hull {
    buckets: Vec<HullIndex>,
    data: HullVec<Node>,

    /// Spare slots in the [`Hull::data`] array, to keep it small
    empty: Vec<HullIndex>,
}

impl Hull {
    pub fn from_triangulation<T>(
        triangulation: &T,
        center: Point2<<T::Vertex as HasPosition>::Scalar>,
    ) -> Self
    where
        T: Triangulation,
    {
        let hull_size = triangulation.convex_hull_size();
        let mut data = HullVec::with_capacity(hull_size);

        assert!(!triangulation.all_vertices_on_line());

        let mut prev_index = HullIndex::new(hull_size - 1);

        for (current_index, edge) in triangulation.convex_hull().enumerate() {
            let angle_from = pseudo_angle(edge.from().position(), center);
            let next_index = HullIndex::new((current_index + 1) % hull_size);

            data.push(Node {
                angle: angle_from,
                vertex: edge.from().fix(),
                left: prev_index,
                right: next_index,
            });
            prev_index = HullIndex::new(current_index);
        }
        let mut result = Self {
            buckets: Vec::new(),
            data,
            empty: Vec::new(),
        };

        const INITIAL_NUMBER_OF_BUCKETS: usize = 8;
        result.initialize_buckets(INITIAL_NUMBER_OF_BUCKETS);

        result
    }

    fn initialize_buckets(&mut self, target_size: usize) {
        self.buckets.clear();
        self.buckets.reserve(target_size);

        const INVALID: HullIndex = HullIndex::const_new(u32::MAX);
        self.buckets
            .extend(std::iter::repeat(INVALID).take(target_size));

        let (first_index, current_node) = self
            .data
            .iter()
            .enumerate()
            .find(|(index, _)| !self.empty.contains(&HullIndex::new(*index)))
            .unwrap();

        let first_index = HullIndex::new(first_index);
        let mut current_index = first_index;
        let first_bucket = self.ceiled_bucket(current_node.angle);
        self.buckets[first_bucket] = current_index;

        loop {
            let current_node = self.data[current_index];
            let segment = self.segment(&current_node);
            let start_bucket = self.ceiled_bucket(segment.from);
            let end_bucket = self.ceiled_bucket(segment.to);
            if start_bucket == end_bucket && segment.is_greater_than_180_degree() {
                // Special case: All buckets point to the same node
                self.buckets.fill(current_index);
                return;
            }

            self.update_bucket_segment(start_bucket, end_bucket, current_index);

            current_index = current_node.right;

            if current_index == first_index {
                break;
            }
        }
    }

    fn insert(
        &mut self,
        left_angle: FloatOrd,
        middle_angle: FloatOrd,
        right_angle: FloatOrd,
        vertex: FixedVertexHandle,
    ) {
        let left_bucket = self.floored_bucket(left_angle);

        let mut left_index = self.buckets[left_bucket];

        loop {
            let current_node = self.data[left_index];
            if current_node.angle == left_angle {
                break;
            }
            left_index = current_node.right;
        }

        let mut right_index = self.data[left_index].right;
        loop {
            let current_node = self.data[right_index];
            if current_node.angle == right_angle {
                break;
            }

            // Remove current_node - it is completely overlapped by the new segment
            self.empty.push(right_index);
            self.data[current_node.left].right = current_node.right;
            self.data[current_node.right].left = current_node.left;
            right_index = current_node.right;
        }

        let new_index = self.get_next_index();

        // Stich the vertex between left_index and right_index
        self.data[left_index].right = new_index;
        self.data[right_index].left = new_index;

        let new_node = Node {
            angle: middle_angle,
            vertex,
            left: left_index,
            right: right_index,
        };
        self.push_or_update_node(new_node, new_index);

        // Update bucket entries appropriately
        let left_bucket = self.ceiled_bucket(left_angle);
        let middle_bucket = self.ceiled_bucket(middle_angle);
        let right_bucket = self.ceiled_bucket(right_angle);

        self.update_bucket_segment(left_bucket, middle_bucket, left_index);
        self.update_bucket_segment(middle_bucket, right_bucket, new_index);

        self.adjust_bucket_size_if_necessary();
    }

    fn get_next_index(&mut self) -> HullIndex {
        self.empty.pop().unwrap_or(HullIndex::new(self.data.len()))
    }

    fn update_bucket_segment(
        &mut self,
        left_bucket: usize,
        right_bucket: usize,
        new_value: HullIndex,
    ) {
        if left_bucket <= right_bucket {
            for current_bucket in &mut self.buckets[left_bucket..right_bucket] {
                *current_bucket = new_value;
            }
        } else {
            // Wrap buckets
            for current_bucket in &mut self.buckets[left_bucket..] {
                *current_bucket = new_value;
            }
            for current_bucket in &mut self.buckets[..right_bucket] {
                *current_bucket = new_value;
            }
        }
    }

    fn push_or_update_node(&mut self, node: Node, index: HullIndex) {
        if let Some(existing_node) = self.data.get_mut(index.0 as usize) {
            *existing_node = node;
        } else {
            assert_eq!(self.data.len(), index.0 as usize);
            self.data.push(node);
        }
    }

    fn get(&self, angle: FloatOrd) -> FixedVertexHandle {
        let mut current_handle = self.buckets[self.floored_bucket(angle)];
        loop {
            let current_node = self.data[current_handle];
            let left_angle = current_node.angle;
            let next_angle = self.data[current_node.right].angle;

            if Segment::new(left_angle, next_angle).contains_angle(angle) {
                return current_node.vertex;
            }

            current_handle = current_node.right;
        }
    }

    /// Looks up what bucket a given pseudo-angle will fall into.
    fn floored_bucket(&self, angle: FloatOrd) -> usize {
        ((angle.0 * (self.buckets.len()) as f64).floor() as usize) % self.buckets.len()
    }

    fn ceiled_bucket(&self, angle: FloatOrd) -> usize {
        ((angle.0 * (self.buckets.len()) as f64).ceil() as usize) % self.buckets.len()
    }

    fn segment(&self, node: &Node) -> Segment {
        Segment::new(node.angle, self.data[node.right].angle)
    }

    fn adjust_bucket_size_if_necessary(&mut self) {
        let size = self.data.len() - self.empty.len();
        let num_buckets = self.buckets.len();

        const MIN_NUMBER_OF_BUCKETS: usize = 16;
        if num_buckets * 2 < size {
            // Too few buckets - increase bucket count
            self.initialize_buckets(num_buckets * 2);
        } else if num_buckets > size * 4 && num_buckets > MIN_NUMBER_OF_BUCKETS {
            let new_size = num_buckets / 4;
            if new_size >= MIN_NUMBER_OF_BUCKETS {
                // Too many buckets - shrink
                self.initialize_buckets(new_size);
            }
        }
    }
}

/// Returns a pseudo-angle in the 0-1 range, without expensive trig functions
///
/// The angle has the following shape:
/// ```text
///              0.25
///               ^ y
///               |
///               |
///   0           |           x
///   <-----------o-----------> 0.5
///   1           |
///               |
///               |
///               v
///              0.75
/// ```
fn pseudo_angle<S: SpadeNum>(a: Point2<S>, center: Point2<S>) -> FloatOrd {
    let a = a.sub(center);
    let a = Point2::<f64>::new(a.x.into(), a.y.into());

    let p = a.x / (a.x.abs() + a.y.abs());
    FloatOrd(1.0 - (if a.y > 0.0 { 3.0 - p } else { 1.0 + p }) / 4.0)
}

#[cfg(test)]
mod test {
    use crate::{
        handles::FixedVertexHandle, triangulation::TriangulationExt, DelaunayTriangulation,
        LastUsedVertexHintGenerator, Point2, Triangulation,
    };

    use super::{FloatOrd, Hull, HullIndex};

    #[test]
    fn test_bulk_load() {
        use crate::test_utilities::{random_points_with_seed, SEED2};

        const SIZE: usize = 9000;
        let mut vertices = random_points_with_seed(SIZE, SEED2);

        vertices.push(Point2::new(4.0, 4.0));
        vertices.push(Point2::new(4.0, -4.0));
        vertices.push(Point2::new(-4.0, 4.0));
        vertices.push(Point2::new(-4.0, -4.0));

        vertices.push(Point2::new(5.0, 5.0));
        vertices.push(Point2::new(5.0, -5.0));
        vertices.push(Point2::new(-5.0, 5.0));
        vertices.push(Point2::new(-5.0, -5.0));

        vertices.push(Point2::new(6.0, 6.0));
        vertices.push(Point2::new(6.0, -6.0));
        vertices.push(Point2::new(-6.0, 6.0));
        vertices.push(Point2::new(-6.0, -6.0));

        let num_vertices = vertices.len();

        let triangulation = DelaunayTriangulation::<Point2<f64>>::bulk_load(vertices);
        triangulation.sanity_check();
        assert_eq!(triangulation.num_vertices(), num_vertices);
    }

    #[test]
    fn test_hull() {
        use super::FloatOrd;

        let mut triangulation =
            DelaunayTriangulation::<_, (), (), (), LastUsedVertexHintGenerator>::new();
        triangulation.insert(Point2::new(1.0, 1.0)); // Angle: 0.375
        triangulation.insert(Point2::new(1.0, -1.0)); // Angle: 0.125
        triangulation.insert(Point2::new(-1.0, 1.0)); // Angle: 0.625
        triangulation.insert(Point2::new(-1.0, -1.0)); // Angle: 0.875

        let mut hull = Hull::from_triangulation(&triangulation, Point2::new(0.0, 0.0));
        sanity_check(&hull);

        let v4 = FixedVertexHandle::new(4);
        hull.insert(FloatOrd(0.375), FloatOrd(0.4), FloatOrd(0.625), v4);
        sanity_check(&hull);

        let v5 = FixedVertexHandle::new(5);
        hull.insert(FloatOrd(0.375), FloatOrd(0.6), FloatOrd(0.125), v5);
        sanity_check(&hull);

        let v6 = FixedVertexHandle::new(6);
        hull.insert(FloatOrd(0.375), FloatOrd(0.4), FloatOrd(0.6), v6);
        sanity_check(&hull);

        let v7 = FixedVertexHandle::new(7);
        hull.insert(FloatOrd(0.4), FloatOrd(0.5), FloatOrd(0.6), v7);
        sanity_check(&hull);
    }

    fn sanity_check(hull: &Hull) {
        let non_empty_nodes: Vec<_> = hull
            .data
            .iter()
            .enumerate()
            .filter(|(index, _)| !hull.empty.contains(&HullIndex::new(*index)))
            .collect();

        for (index, node) in &non_empty_nodes {
            let left_node = hull.data[node.left];
            let right_node = hull.data[node.right];

            assert!(!hull.empty.contains(&node.left));
            assert!(!hull.empty.contains(&node.right));

            assert_eq!(left_node.right, *index);
            assert_eq!(right_node.left, *index);
        }

        for (bucket_index, bucket_node) in hull.buckets.iter().enumerate() {
            let bucket_start_angle = FloatOrd(bucket_index as f64 / hull.buckets.len() as f64);

            for (node_index, node) in &non_empty_nodes {
                let segment = hull.segment(node);

                if segment.contains_angle(bucket_start_angle) {
                    // Make sure the bucket refers to the node with the smallest angle in the same bucket
                    assert_eq!(*node_index, bucket_node.0 as usize);
                }
            }
        }
    }
}