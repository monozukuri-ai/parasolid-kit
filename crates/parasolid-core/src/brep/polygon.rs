//! Ear-clipping triangulation for simple 2D polygons.

/// Computes the signed 2D area of a planar polygon. Positive indicates CCW winding.
fn polygon_signed_area_2d(pts: &[[f64; 2]]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        area += pts[i][0] * pts[j][1] - pts[j][0] * pts[i][1];
    }
    area * 0.5
}

/// Checks if a 2D point `p` lies strictly inside triangle `(a, b, c)`.
fn point_strictly_in_triangle_2d(
    p: [f64; 2],
    a: [f64; 2],
    b: [f64; 2],
    c: [f64; 2],
    ccw: bool,
) -> bool {
    let cross = |p1: [f64; 2], p2: [f64; 2], p3: [f64; 2]| -> f64 {
        (p2[0] - p1[0]) * (p3[1] - p1[1]) - (p2[1] - p1[1]) * (p3[0] - p1[0])
    };
    let c1 = cross(a, b, p);
    let c2 = cross(b, c, p);
    let c3 = cross(c, a, p);
    if ccw {
        c1 > 1e-9 && c2 > 1e-9 && c3 > 1e-9
    } else {
        c1 < -1e-9 && c2 < -1e-9 && c3 < -1e-9
    }
}

/// Robust ear-clipping triangulation for simple (convex or non-convex) 2D polygons.
///
/// Returns a list of vertex index triplets `[i0, i1, i2]` indexing into `polygon`.
/// All emitted triangles have positive (counter-clockwise) winding in the 2D plane.
#[must_use]
pub fn triangulate_polygon_2d(polygon: &[[f64; 2]]) -> Vec<[usize; 3]> {
    let n_in = polygon.len();
    if n_in < 3 {
        return Vec::new();
    }

    // Clean duplicate consecutive vertices and closing vertex
    let mut cleaned_indices = Vec::with_capacity(n_in);
    for i in 0..n_in {
        let pt = polygon[i];
        if let Some(&prev_idx) = cleaned_indices.last() {
            let prev: [f64; 2] = polygon[prev_idx];
            let dist = (pt[0] - prev[0]).hypot(pt[1] - prev[1]);
            if dist > 1e-7 {
                cleaned_indices.push(i);
            }
        } else {
            cleaned_indices.push(i);
        }
    }
    if cleaned_indices.len() >= 2 {
        let first = polygon[cleaned_indices[0]];
        if let Some(&last_index) = cleaned_indices.last() {
            let last = polygon[last_index];
            if (first[0] - last[0]).hypot(first[1] - last[1]) < 1e-7 {
                cleaned_indices.pop();
            }
        }
    }

    let n = cleaned_indices.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        let a = polygon[cleaned_indices[0]];
        let b = polygon[cleaned_indices[1]];
        let c = polygon[cleaned_indices[2]];
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        return if area > 0.0 {
            vec![[cleaned_indices[0], cleaned_indices[1], cleaned_indices[2]]]
        } else {
            vec![[cleaned_indices[0], cleaned_indices[2], cleaned_indices[1]]]
        };
    }

    let cleaned_pts: Vec<[f64; 2]> = cleaned_indices.iter().map(|&i| polygon[i]).collect();
    let area = polygon_signed_area_2d(&cleaned_pts);
    if area.abs() < 1e-12 {
        return Vec::new();
    }
    let ccw = area > 0.0;

    let mut ring: Vec<usize> = (0..n).collect();
    let mut triangles = Vec::with_capacity(n - 2);

    let max_iterations = n * n * 2;
    let mut iteration = 0;

    while ring.len() > 2 && iteration < max_iterations {
        iteration += 1;
        let ring_len = ring.len();
        let mut ear_found = false;

        for i in 0..ring_len {
            let prev = ring[(i + ring_len - 1) % ring_len];
            let curr = ring[i];
            let next = ring[(i + 1) % ring_len];

            let a = cleaned_pts[prev];
            let b = cleaned_pts[curr];
            let c = cleaned_pts[next];

            let cp = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
            let is_convex = if ccw { cp > 1e-9 } else { cp < -1e-9 };

            if !is_convex {
                continue;
            }

            // Check if any other ring vertex is strictly inside this triangle
            let mut has_interior_point = false;
            for &k in &ring {
                if k == prev || k == curr || k == next {
                    continue;
                }
                if point_strictly_in_triangle_2d(cleaned_pts[k], a, b, c, ccw) {
                    has_interior_point = true;
                    break;
                }
            }

            if !has_interior_point {
                // Ear found!
                if ccw {
                    triangles.push([
                        cleaned_indices[prev],
                        cleaned_indices[curr],
                        cleaned_indices[next],
                    ]);
                } else {
                    triangles.push([
                        cleaned_indices[prev],
                        cleaned_indices[next],
                        cleaned_indices[curr],
                    ]);
                }
                ring.remove(i);
                ear_found = true;
                break;
            }
        }

        if !ear_found {
            // Fallback: clip the most convex vertex to avoid infinite looping
            clip_most_convex_vertex(
                &mut ring,
                &cleaned_pts,
                &cleaned_indices,
                ccw,
                &mut triangles,
            );
        }
    }

    triangles
}

fn clip_most_convex_vertex(
    ring: &mut Vec<usize>,
    points: &[[f64; 2]],
    cleaned_indices: &[usize],
    ccw: bool,
    triangles: &mut Vec<[usize; 3]>,
) {
    let ring_len = ring.len();
    let mut best_idx = 0;
    let mut best_cross = if ccw { -f64::INFINITY } else { f64::INFINITY };

    for index in 0..ring_len {
        let prev = ring[(index + ring_len - 1) % ring_len];
        let curr = ring[index];
        let next = ring[(index + 1) % ring_len];
        let a = points[prev];
        let b = points[curr];
        let c = points[next];
        let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        if (ccw && cross > best_cross) || (!ccw && cross < best_cross) {
            best_cross = cross;
            best_idx = index;
        }
    }

    let prev = ring[(best_idx + ring_len - 1) % ring_len];
    let curr = ring[best_idx];
    let next = ring[(best_idx + 1) % ring_len];
    if ccw {
        triangles.push([
            cleaned_indices[prev],
            cleaned_indices[curr],
            cleaned_indices[next],
        ]);
    } else {
        triangles.push([
            cleaned_indices[prev],
            cleaned_indices[next],
            cleaned_indices[curr],
        ]);
    }
    ring.remove(best_idx);
}
