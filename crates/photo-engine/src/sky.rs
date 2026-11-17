//! Finding the sky, without a model: a guess, labelled as one in the app.
//!
//! No small segmentation model with a licence Duckfoot can ship (weights and training data
//! both) was found that knows "sky", and nothing is downloaded. So the sky is grown from
//! the top of the frame through pixels that look like sky (blue, or bright and nearly
//! grey for an overcast one, or warm and bright high in the frame for a sunset) and are
//! smooth, as sky is, and stays close in colour to what it has grown through. Its edge is
//! then softened. It fails on a blue wall reaching the top, a white sky behind white
//! buildings, and night skies; the range mask and invert are there to tidy it.

use image::{GrayImage, RgbImage};

/// How likely each pixel is to be sky, 0..1, and how smooth it is around it.
fn likelihood(img: &RgbImage) -> (Vec<f32>, Vec<f32>) {
    let (w, h) = img.dimensions();
    let px = |x: u32, y: u32| {
        let p = img.get_pixel(x, y).0;
        [
            p[0] as f32 / 255.0,
            p[1] as f32 / 255.0,
            p[2] as f32 / 255.0,
        ]
    };
    let luma = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let mut like = vec![0.0f32; (w * h) as usize];
    let mut smooth = vec![0.0f32; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let c = px(x, y);
            let (hi, lo) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
            let sat = if hi > 0.0 { (hi - lo) / hi } else { 0.0 };
            let y_ = luma(c);
            let hue = {
                let d = (hi - lo).max(1e-6);
                let h = if hi == c[0] {
                    ((c[1] - c[2]) / d).rem_euclid(6.0)
                } else if hi == c[1] {
                    (c[2] - c[0]) / d + 2.0
                } else {
                    (c[0] - c[1]) / d + 4.0
                };
                h * 60.0
            };
            let high = 1.0 - y as f32 / h as f32;
            let blue = (170.0..=260.0).contains(&hue) && sat > 0.08 && y_ > 0.2;
            let overcast = sat < 0.14 && y_ > 0.55;
            let sunset = !(60.0..=300.0).contains(&hue) && y_ > 0.45 && high > 0.55;
            like[(y * w + x) as usize] = if blue || overcast {
                1.0
            } else if sunset {
                0.7
            } else {
                0.0
            };
            // Local contrast against the right and lower neighbours.
            let g = if x + 1 < w && y + 1 < h {
                (y_ - luma(px(x + 1, y))).abs() + (y_ - luma(px(x, y + 1))).abs()
            } else {
                0.0
            };
            smooth[(y * w + x) as usize] = g;
        }
    }
    (like, smooth)
}

/// The sky in `img` (upright, sRGB), as coverage 0..255 the same size.
pub fn sky_coverage(img: &RgbImage) -> GrayImage {
    let (w, h) = img.dimensions();
    let mut out = GrayImage::new(w, h);
    if w < 4 || h < 4 {
        return out;
    }
    let (like, grad) = likelihood(img);
    let colour = |i: usize| {
        let p = img.as_raw();
        [
            p[i * 3] as f32 / 255.0,
            p[i * 3 + 1] as f32 / 255.0,
            p[i * 3 + 2] as f32 / 255.0,
        ]
    };
    // Grown from the top row, through sky-like smooth pixels near the region's colour.
    let mut sky = vec![false; (w * h) as usize];
    let mut queue = std::collections::VecDeque::new();
    let mut sum = [0.0f32; 3];
    let mut n = 0.0f32;
    for x in 0..w {
        let i = x as usize;
        if like[i] > 0.5 && grad[i] < 0.08 {
            sky[i] = true;
            queue.push_back(i);
            let c = colour(i);
            for k in 0..3 {
                sum[k] += c[k];
            }
            n += 1.0;
        }
    }
    while let Some(i) = queue.pop_front() {
        let (x, y) = ((i as u32) % w, (i as u32) / w);
        let mean = sum.map(|s| s / n.max(1.0));
        let near = [
            (x > 0).then(|| i - 1),
            (x + 1 < w).then(|| i + 1),
            (y > 0).then(|| i - w as usize),
            (y + 1 < h).then(|| i + w as usize),
        ];
        for j in near.into_iter().flatten() {
            if sky[j] || like[j] < 0.3 || grad[j] > 0.08 {
                continue;
            }
            let c = colour(j);
            let d =
                ((c[0] - mean[0]).powi(2) + (c[1] - mean[1]).powi(2) + (c[2] - mean[2]).powi(2))
                    .sqrt();
            if d > 0.3 {
                continue;
            }
            sky[j] = true;
            for k in 0..3 {
                sum[k] += c[k];
            }
            n += 1.0;
            queue.push_back(j);
        }
    }
    // A soft edge, about 1% of the long side.
    let hard = GrayImage::from_fn(w, h, |x, y| {
        image::Luma([if sky[(y * w + x) as usize] { 255 } else { 0 }])
    });
    let sigma = (w.max(h) as f32 * 0.01).max(1.0);
    let soft = image::imageops::blur(&hard, sigma);
    for (o, s) in out.pixels_mut().zip(soft.pixels()) {
        *o = *s;
    }
    out
}

/// `img` turned `quarter_turns` clockwise: what the photo looks like upright, for finding
/// a mask in, given the edit's turn.
pub fn turned<P: image::Pixel + 'static>(
    img: &image::ImageBuffer<P, Vec<P::Subpixel>>,
    quarter_turns: u8,
) -> image::ImageBuffer<P, Vec<P::Subpixel>> {
    use image::imageops::{rotate180, rotate270, rotate90};
    match quarter_turns % 4 {
        1 => rotate90(img),
        2 => rotate180(img),
        3 => rotate270(img),
        _ => img.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blue_top_is_sky_and_a_blue_patch_below_is_not() {
        // Blue sky over green ground, and a blue car on the ground.
        let img = RgbImage::from_fn(120, 80, |x, y| {
            if y < 30 {
                image::Rgb([90, 150, 225])
            } else if (60..80).contains(&x) && (55..65).contains(&y) {
                image::Rgb([60, 90, 200])
            } else {
                image::Rgb([60, 110, 50])
            }
        });
        let sky = sky_coverage(&img);
        assert!(sky.get_pixel(60, 5).0[0] > 240, "the sky");
        assert!(sky.get_pixel(10, 70).0[0] < 15, "the ground");
        assert!(sky.get_pixel(70, 60).0[0] < 15, "the car");
    }

    #[test]
    fn turning_four_quarters_is_the_identity() {
        let img = GrayImage::from_fn(5, 3, |x, y| image::Luma([(x * 10 + y) as u8]));
        let once = turned(&img, 1);
        assert_eq!(once.dimensions(), (3, 5));
        assert_eq!(turned(&turned(&once, 1), 2), img);
    }
}
