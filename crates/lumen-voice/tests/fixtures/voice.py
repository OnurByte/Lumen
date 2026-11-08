import json, struct, sys, time
for line in sys.stdin:
    r = json.loads(line)
    if r['text'] == 'hang': time.sleep(60)
    count = 24001 if r['text'] == 'oversize' else 2
    generation = 99 if r['text'] == 'stale' else r['generation']
    sequence = 5 if r['text'] == 'badseq' else 0
    samples = [float('nan'), 0.] if r['text'] == 'nan' else [0.25, -0.5]
    sys.stdout.buffer.write(struct.pack('<4sQQIIIHB', b'PCM1', generation, r['utterance'], sequence, count, 24000, 1, 1))
    sys.stdout.buffer.write(struct.pack('<2f', *samples))
    sys.stdout.buffer.flush()
