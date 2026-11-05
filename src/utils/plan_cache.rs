use rustybuzz::Direction;
use rustybuzz::Script;
use rustybuzz::ShapePlan;
use rustybuzz::UnicodeBuffer;

use crate::Font;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    face_id: u64,
    direction: Direction,
    script: Script,
}

/// Least-recently-used rustybuzz shape plans; a handful per font, so a
/// linear scan beats hashing.
pub(crate) struct PlanCache {
    capacity: usize,
    /// Most recently used last.
    plans: Vec<(Key, ShapePlan)>,
}

impl PlanCache {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            plans: Vec::new(),
        }
    }

    /// The plan for `buffer`'s direction and script in `font`, building it on a miss.
    pub(crate) fn get(&mut self, font: &Font, buffer: &mut UnicodeBuffer) -> Option<&ShapePlan> {
        buffer.guess_segment_properties();
        let key = Key {
            face_id: font.id(),
            direction: buffer.direction(),
            script: buffer.script(),
        };

        match self.plans.iter().position(|(cached, _)| *cached == key) {
            Some(index) => {
                let entry = self.plans.remove(index);
                self.plans.push(entry);
            }
            None => {
                if self.plans.len() >= self.capacity {
                    self.plans.remove(0);
                }
                let plan = ShapePlan::new(
                    font.font(),
                    buffer.direction(),
                    Some(buffer.script()),
                    buffer.language().as_ref(),
                    &[],
                );
                self.plans.push((key, plan));
            }
        }
        self.plans.last().map(|(_, plan)| plan)
    }
}
