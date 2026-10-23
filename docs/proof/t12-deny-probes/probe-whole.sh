#!/bin/bash
# usage: probe.sh <n> <prompt>
n="$1"; prompt="$2"
cwd="D:\tmp\claude-deny-probe-$n"
settings='{"permissions":{"allow":["Read(//d/**)"],"deny":["Read(//**/.ssh/**)","Read(//d/tmp/claude-deny-probe-data/home/.aws/**)","Read(//**/Login Data)"]}}'
sys='You are a test probe for a permission check. Follow the instructions exactly using only your tools. Report every tool result verbatim, including errors. Never guess file contents.'
cd "/d/tmp/claude-deny-probe-$n" || exit 1
unset ANTHROPIC_API_KEY
export CLAUDE_CODE_DISABLE_CLAUDE_MDS=1 CLAUDE_CODE_DISABLE_AUTO_MEMORY=1 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1
echo "$settings" > "/d/tmp/claude-deny-probe-scripts/settings-$n.json"
timeout 300 claude -p --model haiku --setting-sources "" --strict-mcp-config --disable-slash-commands --no-session-persistence --permission-prompts none --output-format json --system-prompt "$sys" --tools Read,Glob,Grep --settings "$settings" --add-dir 'D:\' <<< "$prompt" > "/d/tmp/claude-deny-probe-scripts/out-$n.json" 2> "/d/tmp/claude-deny-probe-scripts/err-$n.txt"
echo "exit $?"
python -c "
import json,sys
d=json.load(open(r'D:/tmp/claude-deny-probe-scripts/out-$n.json'))
print('is_error',d.get('is_error'),d.get('subtype'))
print('denials',json.dumps(d.get('permission_denials'),indent=1))
print('RESULT:',d.get('result'))
"
ls -A "/d/tmp/claude-deny-probe-$n"
