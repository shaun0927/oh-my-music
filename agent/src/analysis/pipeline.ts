// Analysis pipeline orchestrator. Wires Meyda / Essentia / YAMNet
// extractors into a single facade the agent (and LlmContext
// builder) consumes. Concrete extractor implementations are
// injected so tests run deterministically without loading the
// heavyweight WASM/JS libraries.

import {
  deterministicEssentia,
  deterministicMeyda,
  deterministicYamnet,
  type EnvironmentClassification,
  type EssentiaExtractor,
  type FrameFeatures,
  type MeydaExtractor,
  type PcmMono,
  type RhythmKeyEstimate,
  type YamnetClassifier,
} from "./extractors";
import { FeatureStore, type FeatureSnapshot } from "./store";

export type { FeatureSummary, FeatureSnapshot, Trend } from "./store";
export type {
  FrameFeatures,
  EnvironmentClassification,
  RhythmKeyEstimate,
} from "./extractors";

export interface AnalysisPipelineConfig {
  sampleRate: number;
  meyda?: MeydaExtractor;
  essentia?: EssentiaExtractor;
  yamnet?: YamnetClassifier;
}

export interface AnalysisSnapshot {
  features: FeatureSnapshot;
  rhythmKey: RhythmKeyEstimate;
  environment: EnvironmentClassification;
}

export class AnalysisPipeline {
  private readonly meyda: MeydaExtractor;
  private readonly essentia: EssentiaExtractor;
  private readonly yamnet: YamnetClassifier;
  private readonly sampleRate: number;
  private readonly store = new FeatureStore();
  private latestRhythmKey: RhythmKeyEstimate = {
    bpm: 0,
    key: null,
    confidence: 0,
  };
  private latestEnvironment: EnvironmentClassification = {
    label: "unknown",
    confidence: 0,
    speechProbability: 0,
  };

  constructor(config: AnalysisPipelineConfig) {
    this.sampleRate = config.sampleRate;
    this.meyda = config.meyda ?? deterministicMeyda();
    this.essentia = config.essentia ?? deterministicEssentia();
    this.yamnet = config.yamnet ?? deterministicYamnet();
  }

  /// Push a small PCM frame (≈ 50 ms) into the Meyda layer.
  ingestFrame(pcm: PcmMono, timestamp_ms: number): FrameFeatures {
    const features = this.meyda.extract(pcm, this.sampleRate);
    this.store.push(features, timestamp_ms);
    return features;
  }

  /// Re-estimate BPM/key from a longer (≥ 4 s) PCM buffer. Called
  /// periodically, not per frame.
  ingestRhythmKeyWindow(pcm: PcmMono): RhythmKeyEstimate {
    this.latestRhythmKey = this.essentia.estimateRhythmKey(pcm, this.sampleRate);
    return this.latestRhythmKey;
  }

  /// Re-classify the environment from a YAMNet-sized buffer
  /// (~0.96 s). Called periodically.
  ingestEnvironmentWindow(pcm: PcmMono): EnvironmentClassification {
    this.latestEnvironment = this.yamnet.classify(pcm, this.sampleRate);
    return this.latestEnvironment;
  }

  /// Return the current rolled-up snapshot for the LlmContext.
  snapshot(now_ms: number): AnalysisSnapshot {
    return {
      features: this.store.snapshot(now_ms),
      rhythmKey: this.latestRhythmKey,
      environment: this.latestEnvironment,
    };
  }
}
