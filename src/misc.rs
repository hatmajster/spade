// Copyright 2016 The RTree Developers. For a full listing of the authors,
// refer to the Cargo.toml file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use cgmath::{BaseFloat, BaseNum, Vector2};

// A call to l.min(r) does not seem to be inlined, thus we define it ourselves
// This does improve performance significantly, especially for larger node sizes
#[inline]
pub fn fmin<'a, S: BaseFloat>(l: &'a S, r: &'a S) -> &'a S {
    if l < r {
        l
    } else {
        r
    }
}

#[inline]
pub fn fmax<'a, S: BaseFloat>(l: &'a S, r: &'a S) -> &'a S {
    if l > r {
        l
    } else {
        r
    }
}

#[inline]
pub fn clamp<S: BaseNum>(lower: S, upper: S, value: S) -> S {
    upper.partial_min(lower.partial_max(value))
}

pub fn length2<S: BaseNum>(vec: &Vector2<S>) -> S {
    vec.x * vec.x + vec.y * vec.y
}
