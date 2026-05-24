// Rolling-window feature store. Holds per-frame FrameFeatures and
// surfaces last-2-second + last-10-second summaries the
// DecisionLoop's LlmContext consumes.

import type { FrameFeatures } from "./extractors";

export interface FeatureSummary {
  centroidHz: number;
  rmsDb: number;
  onsetRatePerSec: number;
  brightness: "dark" | "neutral" | "bright";
  energy: number; // 0..1, derived from rmsDb
}

export type Trend = "rising" | "falling" | "stable";

export interface FeatureSnapshot {
  last2s: FeatureSummary;
  last10s: FeatureSummary;
  trend: Trend;
}

interface FrameSample {
  timestamp_ms: number;
  features: FrameFeatures;
}

export class FeatureStore {
  private samples: FrameSample[] = [];

  push(features: FrameFeatures, timestamp_ms: number): void {
    this.samples.push({ features, timestamp_ms });
    // Keep ≤ 20 s of history at most so memory stays bounded even
    // at 50 Hz feature rate (1000 samples).
    const cutoff = timestamp_ms - 20_000;
    while (this.samples.length > 0 && this.samples[0].timestamp_ms < cutoff) {
      this.samples.shift();
    }
  }

  snapshot(now_ms: number): FeatureSnapshot {
    const last2 = this.summarize(now_ms - 2000, now_ms);
    const last10 = this.summarize(now_ms - 10_000, now_ms);
    const trend = trendOf(last2, last10);
    return { last2s: last2, last10s: last10, trend };
  }

  size(): number {
    return this.samples.length;
  }

  private summarize(from_ms: number, to_ms: number): FeatureSummary {
    const inWindow = this.samples.filter(
      (s) => s.timestamp_ms >= from_ms && s.timestamp_ms <= to_ms,
    );
    if (inWindow.length === 0) {
      return defaultSummary();
    }
    let sumCentroid = 0;
    let sumRms = 0;
    let onsetCount = 0;
    for (const s of inWindow) {
      sumCentroid += s.features.centroidHz;
      sumRms += s.features.rmsDb;
      if (s.features.onsetThisFrame) onsetCount += 1;
    }
    const n = inWindow.length;
    const centroidHz = sumCentroid / n;
    const rmsDb = sumRms / n;
    const windowSec = Math.max(0.001, (to_ms - from_ms) / 1000);
    const onsetRatePerSec = onsetCount / windowSec;
    return {
      centroidHz,
      rmsDb,
      onsetRatePerSec,
      brightness: brightnessLabel(centroidHz),
      energy: rmsDbToEnergy(rmsDb),
    };
  }
}

function defaultSummary(): FeatureSummary {
  return {
    centroidHz: 0,
    rmsDb: -120,
    onsetRatePerSec: 0,
    brightness: "neutral",
    energy: 0,
  };
}

function trendOf(last2: FeatureSummary, last10: FeatureSummary): Trend {
  const delta = last2.rmsDb - last10.rmsDb;
  if (delta > 1.5) return "rising";
  if (delta < -1.5) return "falling";
  return "stable";
}

function brightnessLabel(centroidHz: number): FeatureSummary["brightness"] {
  if (centroidHz < 1500) return "dark";
  if (centroidHz > 4000) return "bright";
  return "neutral";
}

function rmsDbToEnergy(rmsDb: number): number {
  const lo = -40;
  const hi = -6;
  return Math.max(0, Math.min(1, (rmsDb - lo) / (hi - lo)));
}
