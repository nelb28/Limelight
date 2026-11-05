import os
import soundfile as sf
import sys

AUDIO_EXTS = ('.flac', '.mp3', '.wav', '.ogg')

def convert_flac_to_pcm(input_dir, output_dir):
    """
    Finds all .flac files in the input_dir (recursively),
    and converts them (any sample rate; MP3 needs libsndfile >= 1.1) to raw PCM files in the output_dir.
    """
    if not os.path.exists(input_dir):
        print(f"Error: Input directory '{input_dir}' does not exist.")
        return

    if not os.path.exists(output_dir):
        os.makedirs(output_dir)
        print(f"Created output directory: {output_dir}")

    print(f"Scanning directory: {input_dir}")
    
    count = 0
    skipped = 0
    for root, _, files in os.walk(input_dir):
        for file in files:
            if file.lower().endswith(AUDIO_EXTS):
                flac_path = os.path.join(root, file)
                try:
                    # Determine the relative path to maintain folder structure
                    rel_path = os.path.relpath(root, input_dir)
                    target_folder = os.path.join(output_dir, rel_path)
                    pcm_filename = os.path.splitext(file)[0] + '.pcm'
                    pcm_path = os.path.join(target_folder, pcm_filename)
                    sr_path = os.path.splitext(pcm_path)[0] + '.sr'

                    # The .sr sidecar is written last, so both existing means a
                    # complete earlier conversion (a partial .pcm has no .sr).
                    if os.path.exists(pcm_path) and os.path.exists(sr_path):
                        print(f"Skipping (already converted): {file}")
                        skipped += 1
                        continue

                    os.makedirs(target_folder, exist_ok=True)

                    # Read the file info first to check samplerate without loading data
                    info = sf.info(flac_path)
                        
                    print(f"Converting: {file} ({info.samplerate}Hz) -> {os.path.relpath(pcm_path, output_dir)}")
                        
                    # Read audio data
                    data, samplerate = sf.read(flac_path, always_2d=True)
                    if data.shape[1] == 1:  # cwt expects stereo interleaved
                        data = data.repeat(2, axis=1)
                    elif data.shape[1] > 2:
                        data = data[:, :2]
                        
                    # Write as headerless RAW PCM
                    # Using PCM_24 to match the source bit depth found earlier
                    sf.write(pcm_path, data, samplerate, format='RAW', subtype='PCM_24')
                    # Headerless PCM has no rate info: store it in a sidecar for cwt
                    with open(sr_path, 'w') as f:
                        f.write(str(samplerate))
                    count += 1
                except Exception as e:
                    print(f"Failed to process {file}: {e}")

    print(f"\nFinished. Converted {count} files, skipped {skipped} already converted, in {output_dir}")

if __name__ == "__main__":
    # Default paths relative to this script
    script_dir = os.path.dirname(os.path.abspath(__file__))
    
    default_input_dir = os.path.join(script_dir, "Music")
    default_output_dir = os.path.join(script_dir, "PCM")
    
    input_dir = sys.argv[1] if len(sys.argv) > 1 else default_input_dir
    output_dir = sys.argv[2] if len(sys.argv) > 2 else default_output_dir
    
    convert_flac_to_pcm(input_dir, output_dir)
