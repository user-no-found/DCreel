#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

// Input order is progress first, notification second. All values are physical
// pixels from the same work area; no hard-coded offset assumes 100% scaling.
pub(super) fn arrange(work: Rect, sizes: &[(i32, i32)], margin: i32) -> Vec<Rect> {
    let width = (work.right - work.left).max(1);
    let height = (work.bottom - work.top).max(1);
    let gap = margin.max(0).min(width / 8).min(height / 8);
    let right = work.right - gap;
    let bottom = work.bottom - gap;
    let available_width = (width - 2 * gap).max(1);
    let available_height = (height - 2 * gap).max(1);
    let stacked_height: i32 =
        sizes.iter().map(|size| size.1).sum::<i32>() + gap * sizes.len().saturating_sub(1) as i32;
    let stack = sizes.len() <= 1 || stacked_height <= available_height;
    let column_width = if stack {
        available_width
    } else {
        ((available_width - gap * sizes.len().saturating_sub(1) as i32) / sizes.len() as i32).max(1)
    };
    let mut edge_x = right;
    let mut edge_y = bottom;
    sizes
        .iter()
        .map(|&(width, height)| {
            let width = width.max(1).min(column_width);
            let height = height.max(1).min(available_height);
            let rect = Rect {
                left: edge_x - width,
                top: edge_y - height,
                right: edge_x,
                bottom: edge_y,
            };
            if stack {
                edge_y = rect.top - gap;
            } else {
                edge_x = rect.left - gap;
            }
            rect
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_layout_stays_inside_work_area_without_overlap() {
        for work in [
            Rect {
                left: 0,
                top: 0,
                right: 2880,
                bottom: 1740,
            },
            Rect {
                left: -1920,
                top: -200,
                right: 0,
                bottom: 840,
            },
            Rect {
                left: 0,
                top: 0,
                right: 800,
                bottom: 440,
            },
        ] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let rectangles = arrange(
                    work,
                    &[
                        ((420.0 * scale) as i32, (226.0 * scale) as i32),
                        ((380.0 * scale) as i32, (166.0 * scale) as i32),
                    ],
                    (18.0 * scale) as i32,
                );
                for rect in &rectangles {
                    assert!(rect.left >= work.left && rect.top >= work.top);
                    assert!(rect.right <= work.right && rect.bottom <= work.bottom);
                }
                let (a, b) = (rectangles[0], rectangles[1]);
                assert!(
                    a.left >= b.right
                        || b.left >= a.right
                        || a.top >= b.bottom
                        || b.top >= a.bottom
                );
            }
        }
    }

    #[test]
    fn remaining_popup_returns_to_bottom_right() {
        let work = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let rect = arrange(work, &[(380, 166)], 18)[0];
        assert_eq!(rect.right, 1902);
        assert_eq!(rect.bottom, 1022);
    }
}
