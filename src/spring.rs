/// Critically-damped / spring-damper scalar interpolator for smooth iOS-style motion.
#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Spring {
    pub fn new(initial: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            value: initial,
            target: initial,
            velocity: 0.0,
            stiffness,
            damping,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }

    /// Advances the spring by `dt` seconds using semi-implicit Euler sub-steps.
    /// Returns `true` if the spring is still animating.
    pub fn step(&mut self, dt: f32) -> bool {
        let dt = dt.clamp(0.0005, 0.05);
        let substeps = 2;
        let sub_dt = dt / substeps as f32;

        for _ in 0..substeps {
            let force = (self.target - self.value) * self.stiffness;
            let accel = force - self.velocity * self.damping;
            self.velocity += accel * sub_dt;
            self.value += self.velocity * sub_dt;
        }

        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
            false
        } else {
            true
        }
    }

    pub fn is_settled(&self) -> bool {
        (self.target - self.value).abs() < 0.35 && self.velocity.abs() < 1.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_converges_to_target_and_settles() {
        let mut s = Spring::new(44.0, 290.0, 30.0);
        s.set_target(360.0);

        let mut steps = 0;
        while s.step(1.0 / 60.0) {
            steps += 1;
            assert!(steps < 300, "spring failed to settle in reasonable time");
        }

        assert!((s.value - 360.0).abs() < f32::EPSILON);
        assert!(s.is_settled());
    }
}
