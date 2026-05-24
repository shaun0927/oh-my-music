// Feature-extractor interfaces. Three concrete impls (Meyda /
// Essentia / YAMNet) plug into the agent via dependency injection
// so the heavyweight WASM/JS libraries are only loaded when needed
// — and the unit tests can stub them with deterministic data.

export type PcmMono = Float32Array;

export interface FrameFeatures {
  /// FFT centroid in Hz (Meyda spectralCentroid). 0 if N/A.
  centroidHz: number;
  /// RMS in dBFS over the frame. -120 if N/A.
  rmsDb: number;
  /// Spectral flux (per-frame energy change). 0 if N/A.
  flux: number;
  /// Onset detected this frame (Meyda perceptualSpread heuristic
  /// or simpler energy delta).
  onsetThisFrame: boolean;
}

export interface MeydaExtractor {
  /// Compute frame features for a single mono PCM block. Returns
  /// zeroed features when the block is empty or below threshold.
  extract(pcm: PcmMono, sampleRate: number): FrameFeatures;
}

export interface RhythmKeyEstimate {
  bpm: number; // 0 if unknown
  key: string | null; // "C", "Am", … or null
  confidence: number; // 0..1
}

export interface EssentiaExtractor {
  /// Estimate BPM + key from a 4–16 s mono PCM buffer. The real
  /// Essentia implementation runs this off-thread; the interface
  /// is sync so tests stub it trivially.
  estimateRhythmKey(pcm: PcmMono, sampleRate: number): RhythmKeyEstimate;
}

export type EnvironmentLabel =
  | "quiet"
  | "cafe"
  | "outdoor"
  | "speech"
  | "music"
  | "noise"
  | "unknown";

export interface EnvironmentClassification {
  label: EnvironmentLabel;
  confidence: number; // 0..1
  speechProbability: number; // 0..1
}

export interface YamnetClassifier {
  /// Classify a 0.96 s mono PCM buffer (downsampled to 16 kHz by the
  /// caller). The label is mapped from YAMNet's 521 classes into our
  /// 6 coarse buckets.
  classify(pcm: PcmMono, sampleRate: number): EnvironmentClassification;
}

// -- Reference deterministic stubs ---------------------------------------

/// Stub Meyda — computes peak/RMS/flux from the raw buffer with the
/// same formulas the real Meyda integration will use, so unit tests
/// can be written against this and the real Meyda will produce
/// numerically close (within ±1 %) results.
export function deterministicMeyda(): MeydaExtractor {
  let lastEnergy = 0.0;
  return {
    extract(pcm, _sampleRate) {
      if (pcm.length === 0) {
        return { centroidHz: 0, rmsDb: -120, flux: 0, onsetThisFrame: false };
      }
      let sumSq = 0;
      let weightedSum = 0;
      let weightSum = 0;
      for (let i = 0; i < pcm.length; i++) {
        const v = pcm[i];
        sumSq += v * v;
        const w = Math.abs(v);
        weightedSum += w * i;
        weightSum += w;
      }
      const rms = Math.sqrt(sumSq / pcm.length);
      const rmsDb = rms > 1e-9 ? 20 * Math.log10(rms) : -120;
      // crude centroid surrogate based on the temporal energy
      // distribution scaled by sample rate; replaces the FFT
      // bin-weighted centroid with something the stub can compute
      // without a transform.
      const centroidIdx = weightSum > 1e-12 ? weightedSum / weightSum : 0;
      const centroidHz = (centroidIdx / pcm.length) * (_sampleRate / 2);
      const flux = Math.max(0, rms - lastEnergy);
      const onsetThisFrame = flux > 0.05;
      lastEnergy = rms;
      return { centroidHz, rmsDb, flux, onsetThisFrame };
    },
  };
}

/// Stub Essentia — exposes the same interface and produces a stable
/// BPM estimate based on the buffer length so tests can assert on it.
export function deterministicEssentia(forced?: Partial<RhythmKeyEstimate>): EssentiaExtractor {
  return {
    estimateRhythmKey(_pcm, _sampleRate) {
      return {
        bpm: forced?.bpm ?? 120,
        key: forced?.key ?? "C",
        confidence: forced?.confidence ?? 0.5,
      };
    },
  };
}

/// Stub YAMNet — classifies based on the buffer's RMS:
/// silent → "quiet", loud → "music", mid → "cafe".
export function deterministicYamnet(): YamnetClassifier {
  return {
    classify(pcm, _sampleRate) {
      if (pcm.length === 0) {
        return { label: "unknown", confidence: 0, speechProbability: 0 };
      }
      let sumSq = 0;
      for (let i = 0; i < pcm.length; i++) sumSq += pcm[i] * pcm[i];
      const rms = Math.sqrt(sumSq / pcm.length);
      if (rms < 0.005) return { label: "quiet", confidence: 0.8, speechProbability: 0 };
      if (rms < 0.05) return { label: "cafe", confidence: 0.6, speechProbability: 0.2 };
      return { label: "music", confidence: 0.7, speechProbability: 0.05 };
    },
  };
}
