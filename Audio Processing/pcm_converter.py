import os
import soundfile as sf
import sys

def convert_flac_to_pcm(input_dir, output_dir):
    """
    Finds all .flac files in the input_dir (recursively),
    checks if they are 96kHz, and converts them to raw PCM files in the output_dir.
    """
    if not os.path.exists(input_dir):
        print(f"Error: Input directory '{input_dir}' does not exist.")
        return

    if not os.path.exists(output_dir):
        os.makedirs(output_dir)
        print(f"Created output directory: {output_dir}")

    print(f"Scanning directory: {input_dir}")
    
    count = 0
    for root, _, files in os.walk(input_dir):
        for file in files:
            if file.lower().endswith('.flac'):
                flac_path = os.path.join(root, file)
                try:
                    # Read the file info first to check samplerate without loading data
                    info = sf.info(flac_path)
                    
                    if info.samplerate == 96000 and info.subtype == 'PCM_24':
                        # Determine the relative path to maintain folder structure
                        rel_path = os.path.relpath(root, input_dir)
                        target_folder = os.path.join(output_dir, rel_path)
                        
                        if not os.path.exists(target_folder):
                            os.makedirs(target_folder)
                        
                        pcm_filename = os.path.splitext(file)[0] + '.pcm'
                        pcm_path = os.path.join(target_folder, pcm_filename)
                        
                        print(f"Converting: {file} (96kHz) -> {os.path.relpath(pcm_path, output_dir)}")
                        
                        # Read audio data
                        data, samplerate = sf.read(flac_path)
                        
                        # Write as headerless RAW PCM
                        # Using PCM_24 to match the source bit depth found earlier
                        sf.write(pcm_path, data, samplerate, format='RAW', subtype='PCM_24')
                        count += 1
                except Exception as e:
                    print(f"Failed to process {file}: {e}")

    print(f"\nFinished. Converted {count} files to {output_dir}")

if __name__ == "__main__":
    # Default paths relative to this script
    script_dir = os.path.dirname(os.path.abspath(__file__))
    
    default_input_dir = os.path.join(script_dir, "Music")
    default_output_dir = os.path.join(script_dir, "PCM")
    
    input_dir = sys.argv[1] if len(sys.argv) > 1 else default_input_dir
    output_dir = sys.argv[2] if len(sys.argv) > 2 else default_output_dir
    
    convert_flac_to_pcm(input_dir, output_dir)
