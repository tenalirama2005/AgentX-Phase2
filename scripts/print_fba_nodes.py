import sys, json
d = json.load(sys.stdin)
content = json.loads(d.get('content','{}'))
nodes = sorted(content.get('nodes',[]), key=lambda x: x.get('confidence',0), reverse=True)
for i, n in enumerate(nodes, 1):
    conf = n.get('confidence', 0)
    icon = '[OK]' if conf >= 0.85 else '[WARN]' if conf >= 0.70 else '[LOW]'
    print(f'  [{i:02d}] {n["node_id"]:<35} {conf:.1%}  {icon}')
