import { describe, expect, test } from "bun:test";

import {
  deterministicEssentia,
  deterministicMeyda,
  deterministicYamnet,
} from "../extractors";
import { FeatureStore } from "../store";
import { AnalysisPipeline } from "../pipeline";

function sine(n: number, sampleRate: number, freq: number, amp: number): Float32Array {
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    out[i] = amp * Math.sin((2 * Math.PI * freq * i) / sampleRate);
  }
  return out;
}

describe("deterministicMeyda", () => {
  test("silence yields RMS -120 dB and no onset", () => {
    const meyda = deterministicMeyda();
    const f = meyda.extract(new Float32Array(1024), 48_000);
    expect(f.rmsDb).toBe(-120);
    expect(f.onsetThisFrame).toBe(false);
  });

  test("loud sine yields an audible RMS", () => {
    const meyda = deterministicMeyda();
    const buf = sine(1024, 48_000, 440, 0.5);
    const f = meyda.extract(buf, 48_000);
    // 0.5 amplitude sine RMS = 0.5/√2 ≈ 0.354 → -9 dB
    expect(f.rmsDb).toBeGreaterThan(-12);
    expect(f.rmsDb).toBeLessThan(-6);
  });
});

describe("deterministicYamnet", () => {
  test("silent buffer is 'quiet'", () => {
    const y = deterministicYamnet();
    const c = y.classify(new Float32Array(960), 16_000);
    expect(c.label).toBe("quiet");
  });

  test("loud buffer is 'music'", () => {
    const y = deterministicYamnet();
    const c = y.classify(sine(960, 16_000, 440, 0.6), 16_000);
    expect(c.label).toBe("music");
  });
});

describe("deterministicEssentia", () => {
  test("returns the forced BPM/key when overridden", () => {
    const e = deterministicEssentia({ bpm: 140, key: "Am", confidence: 0.9 });
    const r = e.estimateRhythmKey(new Float32Array(0), 48_000);
    expect(r.bpm).toBe(140);
    expect(r.key).toBe("Am");
    expect(r.confidence).toBe(0.9);
  });
});

describe("FeatureStore", () => {
  test("snapshot on empty store returns default summary", () => {
    const s = new FeatureStore();
    const snap = s.snapshot(1000);
    expect(snap.last2s.rmsDb).toBe(-120);
    expect(snap.trend).toBe("stable");
  });

  test("push then snapshot reflects the pushed frames", () => {
    const s = new FeatureStore();
    s.push(
      { centroidHz: 2000, rmsDb: -12, flux: 0.1, onsetThisFrame: true },
      0,
    );
    s.push(
      { centroidHz: 2200, rmsDb: -10, flux: 0.2, onsetThisFrame: false },
      500,
    );
    const snap = s.snapshot(1000);
    expect(snap.last2s.centroidHz).toBeCloseTo(2100, 0);
    expect(snap.last2s.rmsDb).toBeCloseTo(-11, 0);
    expect(snap.last2s.brightness).toBe("neutral");
  });

  test("trend reports 'rising' when 2s is louder than 10s", () => {
    const s = new FeatureStore();
    // 5s of -20 dB
    for (let t = 0; t < 5000; t += 100) {
      s.push({ centroidHz: 1000, rmsDb: -20, flux: 0, onsetThisFrame: false }, t);
    }
    // last 1s much louder
    for (let t = 5000; t < 6000; t += 100) {
      s.push({ centroidHz: 1000, rmsDb: -8, flux: 0, onsetThisFrame: false }, t);
    }
    expect(s.snapshot(6000).trend).toBe("rising");
  });

  test("old samples are evicted past 20s", () => {
    const s = new FeatureStore();
    s.push({ centroidHz: 1000, rmsDb: -30, flux: 0, onsetThisFrame: false }, 0);
    s.push(
      { centroidHz: 1000, rmsDb: -30, flux: 0, onsetThisFrame: false },
      25_000,
    );
    expect(s.size()).toBe(1);
  });
});

describe("AnalysisPipeline", () => {
  test("ingest + snapshot round-trips per-frame features", () => {
    const p = new AnalysisPipeline({ sampleRate: 48_000 });
    p.ingestFrame(sine(1024, 48_000, 440, 0.5), 0);
    p.ingestFrame(sine(1024, 48_000, 440, 0.5), 100);
    const snap = p.snapshot(200);
    expect(snap.features.last2s.rmsDb).toBeGreaterThan(-15);
  });

  test("rhythm/key ingest updates the snapshot", () => {
    const p = new AnalysisPipeline({
      sampleRate: 48_000,
      essentia: deterministicEssentia({ bpm: 95, key: "F#m", confidence: 0.7 }),
    });
    p.ingestRhythmKeyWindow(new Float32Array(48_000 * 5));
    const snap = p.snapshot(1000);
    expect(snap.rhythmKey.bpm).toBe(95);
    expect(snap.rhythmKey.key).toBe("F#m");
  });

  test("environment ingest updates the snapshot", () => {
    const p = new AnalysisPipeline({ sampleRate: 16_000 });
    p.ingestEnvironmentWindow(sine(15_360, 16_000, 440, 0.6));
    const snap = p.snapshot(0);
    expect(snap.environment.label).toBe("music");
  });

  test("custom extractors are injected and used", () => {
    let extractCalled = 0;
    const p = new AnalysisPipeline({
      sampleRate: 48_000,
      meyda: {
        extract() {
          extractCalled += 1;
          return { centroidHz: 0, rmsDb: -60, flux: 0, onsetThisFrame: false };
        },
      },
    });
    p.ingestFrame(new Float32Array(1024), 0);
    expect(extractCalled).toBe(1);
  });
});
