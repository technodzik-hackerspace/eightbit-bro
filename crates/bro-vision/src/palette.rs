use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::UNSURE;

/// The colours the console actually produces through the capture card, learned from
/// fixtures (tech design §3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    pub colors: Vec<[u8; 3]>,
    /// A sample further than this (per channel) from every colour is `UNSURE`.
    pub max_dist: u8,
}

impl Palette {
    /// Learns the palette from sampled pixels: colours seen at least `min_count` times,
    /// merging shades closer than `max_dist` per channel (JPEG noise). Rare colours are
    /// edge ringing and are left out, so they come out `UNSURE`.
    pub fn learn<'a>(
        samples: impl IntoIterator<Item = &'a [u8; 3]>,
        min_count: u32,
        max_dist: u8,
    ) -> Palette {
        let mut bins: HashMap<[u8; 3], (u32, [u64; 3])> = HashMap::new();
        for c in samples {
            let bin = bins.entry(c.map(|v| v >> 3)).or_default();
            bin.0 += 1;
            for (s, v) in bin.1.iter_mut().zip(c) {
                *s += *v as u64;
            }
        }
        let mut bins: Vec<_> = bins.into_iter().collect();
        // Biggest first, ties by bin key: the result must not depend on hash order.
        bins.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(&b.0)));

        let mut clusters: Vec<(u32, [u64; 3])> = Vec::new();
        for (_, (count, sum)) in bins.into_iter().filter(|(_, (n, _))| *n >= min_count) {
            let color = mean(count, sum);
            match clusters
                .iter_mut()
                .find(|(n, s)| dist(mean(*n, *s), color) <= max_dist)
            {
                Some((n, s)) => {
                    *n += count;
                    for (s, v) in s.iter_mut().zip(sum) {
                        *s += v;
                    }
                }
                None => clusters.push((count, sum)),
            }
        }
        Palette {
            colors: clusters.into_iter().map(|(n, s)| mean(n, s)).collect(),
            max_dist,
        }
    }

    pub fn quantize(&self, c: [u8; 3]) -> u8 {
        self.colors
            .iter()
            .enumerate()
            .map(|(i, p)| (dist(*p, c), i))
            .min()
            .filter(|(d, _)| *d <= self.max_dist)
            .map_or(UNSURE, |(_, i)| i as u8)
    }
}

/// Largest per-channel difference.
pub(crate) fn dist(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap()
}

fn mean(count: u32, sum: [u64; 3]) -> [u8; 3] {
    sum.map(|s| (s / count as u64) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NesCanvas, fixtures};

    #[test]
    fn merges_noisy_shades_and_drops_rare_colours() {
        let samples = [[10, 10, 10]; 50]
            .into_iter()
            .chain([[14, 12, 9]; 50])
            .chain([[200, 30, 30]; 40])
            .chain([[90, 200, 90]; 2])
            .collect::<Vec<_>>();
        let p = Palette::learn(&samples, 10, 24);
        assert_eq!(p.colors.len(), 2);
        assert_eq!(p.quantize([12, 11, 9]), p.quantize([10, 10, 10]));
        assert_ne!(p.quantize([200, 30, 30]), UNSURE);
        assert_eq!(p.quantize([90, 200, 90]), UNSURE);
    }

    #[test]
    fn tetris_frame_quantizes_mostly_sure() {
        let (img, geo, canvas) = fixtures::tetris_1p();
        let palette = Palette::learn(canvas.rgb.iter(), 20, 40);
        assert!(
            (5..=32).contains(&palette.colors.len()),
            "{} colours",
            palette.colors.len()
        );
        let canvas = NesCanvas::sample(&img, &geo, Some(&palette));
        let unsure = canvas.px.iter().filter(|&&p| p == UNSURE).count();
        assert!(unsure * 100 < canvas.px.len() * 5, "{unsure} unsure pixels");
    }
}
