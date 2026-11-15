//! Cell-coordinate geometry primitives.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> u16 {
        self.x.saturating_add(self.width)
    }

    pub fn bottom(&self) -> u16 {
        self.y.saturating_add(self.height)
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x
            && point.x < self.right()
            && point.y >= self.y
            && point.y < self.bottom()
    }

    pub fn intersects(&self, other: Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// Clamp a width request to at most `max`.
    pub fn clamp_width(&self, max: u16) -> u16 {
        self.width.min(max)
    }
}

/// Split `width` into `count` near-equal columns; the last column absorbs the
/// remainder so columns always sum to exactly `width`.
pub fn distribute_width(width: u16, count: usize) -> Vec<u16> {
    if count == 0 {
        return Vec::new();
    }
    let base = width / count.max(1) as u16;
    let mut out = vec![base; count];
    let remainder = width - base * count as u16;
    if let Some(last) = out.last_mut() {
        *last += remainder;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distributes_remainder_to_last_column() {
        assert_eq!(distribute_width(10, 3), vec![3, 3, 4]);
        assert_eq!(distribute_width(5, 2), vec![2, 3]);
    }

    #[test]
    fn rect_containment() {
        let rect = Rect::new(1, 1, 3, 3);
        assert!(rect.contains(Point { x: 1, y: 1 }));
        assert!(rect.contains(Point { x: 3, y: 3 }));
        assert!(!rect.contains(Point { x: 4, y: 1 }));
    }
}
