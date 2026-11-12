//! Effects bound to a dancer's own joints (Winx Club, research
//! `docs/wii_ddr_winx_club_research.md` §3 / §7.4): the Enchantix
//! transformation's `chenge_effect` (spawned at the wipe line, faded out at
//! the pose) and the Fairy dancers' wing-tip sparkle trail
//! (`chara_effects`) — one effect of one bank, `instances` copies, each at
//! its own attach frame, run by `flight_fx`'s CzanEff port and drawn through
//! the bank's POOL models (`port_winx_fx.py`, layout = `flight_fx.txt`'s
//! grammar, every pool player 0). PURE (std + `super::flight_fx`;
//! harness-mounted as `chara_fx`).
//!
//! Content (a Fairy dancer's body folder): `winx_fx/chenge.{teb,txt}`,
//! `winx_fx/wing.{teb,txt}` + the `fx_<key>_c*` / `fx_<key>_w*` pool models.

use super::flight_fx::{
    Affine, Camera, EffectSim, FxLayout, Pool, PoolFrame, PoolWriter, Teb, MAX_POOLS,
};

/// The pool's player in a Winx layout.
pub const PLAYER: usize = 0;

/// One effect × `instances` at their own attach frames.
pub struct JointFx {
    /// Pool indices into the layout (= `frames` order).
    pub pools: Vec<usize>,
    pub frames: Vec<PoolFrame>,
    sims: Vec<EffectSim>,
    start: Option<f32>,
    /// Drawables that found no pool room (diagnostics).
    pub dropped: usize,
}

impl JointFx {
    /// `None` without the effect or a pool.
    pub fn new(
        teb: &Teb,
        layout: &FxLayout,
        effect: usize,
        instances: usize,
        seed: u32,
    ) -> Option<JointFx> {
        let mut pools = layout.pools_of(PLAYER);
        pools.truncate(MAX_POOLS);
        if pools.is_empty() {
            return None;
        }
        let frames = pools
            .iter()
            .filter_map(|&i| layout.pools.get(i))
            .map(PoolFrame::new)
            .collect();
        let sims = (0..instances.max(1))
            .map(|k| EffectSim::new(teb, effect, seed ^ (k as u32).wrapping_mul(0x9E37_79B9)))
            .collect::<Option<Vec<_>>>()?;
        Some(JointFx {
            pools,
            frames,
            sims,
            start: None,
            dropped: 0,
        })
    }

    /// Back to "never started" (nothing alive).
    pub fn reset(&mut self, teb: &Teb) {
        self.start = None;
        for s in self.sims.iter_mut() {
            s.reset(teb);
        }
        for f in self.frames.iter_mut() {
            f.clear();
        }
    }

    pub fn running(&self) -> bool {
        self.start.is_some()
    }

    /// One frame: `on` = the effect plays (started at the first `on` frame;
    /// `off` resets it), `real_s` the real clock, `attaches[k]` instance
    /// k's frame (zan units; the last one repeats), `fade` 0..1 scales the
    /// drawn colours (additive pools: rgb). Refills `frames`; returns the
    /// entries drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        teb: &Teb,
        layout: &FxLayout,
        on: bool,
        real_s: f32,
        attaches: &[Affine],
        cam: &Camera,
        fade: f32,
    ) -> usize {
        for f in self.frames.iter_mut() {
            f.clear();
        }
        if !on || attaches.is_empty() {
            if self.start.is_some() {
                self.reset(teb);
            }
            return 0;
        }
        let start = *self.start.get_or_insert(real_s);
        let mut elapsed = real_s - start;
        if !elapsed.is_finite() || elapsed < 0.0 {
            // the clock went back: restart here
            self.reset(teb);
            self.start = Some(real_s);
            elapsed = 0.0;
        }
        for (k, sim) in self.sims.iter_mut().enumerate() {
            let a = attaches
                .get(k)
                .or_else(|| attaches.last())
                .copied()
                .unwrap_or(Affine::IDENTITY);
            sim.advance(teb, elapsed, &a, cam, [0.0; 3]);
        }
        let empty = Pool {
            player: PLAYER,
            model: String::new(),
            kind: super::flight_fx::PoolKind::Sprites {
                quads: 0,
                groups: Vec::new(),
            },
        };
        let mut flat: [&Pool; MAX_POOLS] = [&empty; MAX_POOLS];
        for (dst, &i) in flat.iter_mut().zip(self.pools.iter()) {
            if let Some(p) = layout.pools.get(i) {
                *dst = p;
            }
        }
        let n = self.pools.len().min(self.frames.len()).min(MAX_POOLS);
        let mut w = PoolWriter {
            pools: &flat[..n],
            frames: &mut self.frames[..n],
            metres_per_unit: layout.metres_per_unit,
            dropped: 0,
        };
        for sim in &self.sims {
            sim.emit(teb, &mut w);
        }
        self.dropped += w.dropped;
        let f = fade.clamp(0.0, 1.0);
        if f < 1.0 {
            for fr in self.frames.iter_mut() {
                for c in fr.colours.iter_mut() {
                    c[0] *= f;
                    c[1] *= f;
                    c[2] *= f;
                }
            }
        }
        self.frames.iter().map(|f| f.drawn).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_pool_means_no_effect() {
        let layout = FxLayout::parse("flight_fx 1\nmetres_per_unit 0.1\n").unwrap();
        let teb = Teb {
            effects: Vec::new(),
        };
        assert!(JointFx::new(&teb, &layout, 0, 2, 1).is_none());
    }
}
