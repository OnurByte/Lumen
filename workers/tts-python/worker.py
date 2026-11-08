"""Persistent Turkish TTS worker: bounded JSON stdin, framed float32 PCM stdout.
Cancellation is process termination; the Rust adapter owns restart and generation IDs.
"""
import array
import contextlib
import json
import math
import struct
import sys

REVISION = 'fcbd37c74dc127be846face0ff36062e9a939f7b'
VOICES = {'female_1', 'female_2', 'female_3', 'male_1', 'male_2', 'male_3'}
HEADER = struct.Struct('<4sQQIIIHB')

def validate_request(request):
    if not isinstance(request, dict) or set(request) != {'generation', 'utterance', 'text', 'voice'}:
        raise ValueError('invalid request fields')
    if any(type(request[key]) is not int or not 0 <= request[key] < 2**64 for key in ('generation', 'utterance')):
        raise ValueError('invalid identity')
    if not isinstance(request['text'], str) or not request['text'].strip() or len(request['text'].encode()) > 4096:
        raise ValueError('invalid text')
    if request['voice'] not in VOICES:
        raise ValueError('unknown synthetic voice')
    return request

def encode_frame(generation, utterance, sequence, last, samples):
    if not 0 < len(samples) <= 24000 or any(not math.isfinite(x) or abs(x) > 1 for x in samples):
        raise ValueError('invalid PCM samples')
    pcm = array.array('f', samples)
    if sys.byteorder != 'little': pcm.byteswap()
    return HEADER.pack(b'PCM1', generation, utterance, sequence, len(pcm), 24000, 1, int(last)) + pcm.tobytes()

def main():
    binary = sys.stdout.buffer
    with contextlib.redirect_stdout(sys.stderr):
        from pocket_tts_turkish import TurkishTTS
        import torch
        torch.set_num_threads(2)
        model = TurkishTTS.from_pretrained(device='cpu', revision=REVISION)
    while True:
        line = sys.stdin.buffer.readline(8193)
        if not line: return
        if len(line) > 8192 or not line.endswith(b'\n'): raise ValueError('request too large or truncated')
        request = validate_request(json.loads(line))
        with contextlib.redirect_stdout(sys.stderr):
            chunks = iter(model.stream(request['text'], voice=request['voice']))
            previous = next(chunks, None)
            if previous is None: raise ValueError('TTS returned no audio')
            sequence = 0
            for current in chunks:
                binary.write(encode_frame(request['generation'], request['utterance'], sequence, False, previous))
                binary.flush()
                previous = current
                sequence += 1
            binary.write(encode_frame(request['generation'], request['utterance'], sequence, True, previous))
            binary.flush()

if __name__ == '__main__':
    try: main()
    except Exception as error:
        print(f'TTS worker failed: {type(error).__name__}: {error}', file=sys.stderr)
        sys.exit(1)
