#!/usr/bin/env python3
"""Opt-in live PipeWire test: two Linux agents, real PCM, synthetic microphones.

Requires the normal LAN Mouse daemon to be running for the liveness check.
Pauses/restores lan-mic.service, refuses to run alongside recording apps,
uses private fixture logs, and restores original audio defaults in finally.
No real microphone recording and no changes to LAN Mouse's actual logs.
"""
import argparse,array,json,math,os,socket,struct,subprocess,tempfile,threading,time
from pathlib import Path

def pulse(*args):return subprocess.check_output(['pactl',*args],text=True).strip()
def wait_for(test,timeout=8):
 deadline=time.monotonic()+timeout
 while time.monotonic()<deadline:
  try:
   if test():return
  except (OSError,ValueError,KeyError):pass
  time.sleep(.05)
 raise AssertionError('Condition did not become true')
def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--binary',required=True);args=parser.parse_args()
 binary=str(Path(args.binary).resolve());before=pulse('get-default-source');sink_before=pulse('get-default-sink')
 streams=json.loads(pulse('-f','json','list','source-outputs'))
 external=[s for s in streams if s.get('properties',{}).get('node.name')!='input.lan_mic']
 if external:raise SystemExit('Close recording apps before running this live audio test.')
 was_active=subprocess.run(['systemctl','--user','is-active','--quiet','lan-mic']).returncode==0
 processes=[];modules=[];connections=[];stop=threading.Event();threads=[];received=bytearray()
 if was_active:subprocess.run(['systemctl','--user','stop','lan-mic'],check=True)
 try:
  modules.append(pulse('load-module','module-null-sink','sink_name=lan_mic_testtone','rate=48000','channels=1','channel_map=mono'))
  modules.append(pulse('load-module','module-remap-source','master=lan_mic_testtone.monitor','source_name=lan_mic_test_input','channels=1','channel_map=mono'))
  pulse('set-default-source',before);pulse('set-default-sink',sink_before)
  pulse('set-sink-mute','lan_mic_testtone','0');pulse('set-source-mute','lan_mic_test_input','0')
  pulse('set-sink-volume','lan_mic_testtone','100%');pulse('set-source-volume','lan_mic_test_input','100%')
  with tempfile.TemporaryDirectory(prefix='lan-mic-linux-test-')as directory:
   root=Path(directory);nodes=[]
   for name in ['Alpha','Beta']:
    node=root/name;state=node/'state/lan-mic';conf=node/'config/lan-mic';state.mkdir(parents=True);conf.mkdir(parents=True);log=state/'lan-mouse.log';log.write_text('')
    (conf/'config.json').write_text(json.dumps({'computer_name':name,'input_source':'lan_mic_test_input','lan_mouse_log':str(log),'target_client':0}))
    env={**os.environ,'XDG_STATE_HOME':str(node/'state'),'XDG_CONFIG_HOME':str(node/'config')}
    err=(node/'stderr.log').open('w');processes.append(subprocess.Popen([binary,'agent'],env=env,stderr=err));nodes.append((node,state,log))
   for _,state,_ in nodes:
    wait_for(lambda s=state:(s/'audio.sock').exists());connection=socket.socket(socket.AF_UNIX);connection.connect(str(state/'audio.sock'));connections.append(connection)
   def bridge(source,target):
    try:
     while not stop.is_set():
      data=source.recv(65536)
      if not data:break
      target.sendall(data)
    except OSError:pass
   for a,b in [(0,1),(1,0)]:
    thread=threading.Thread(target=bridge,args=(connections[a],connections[b]),daemon=True);thread.start();threads.append(thread)
   def status(index):return json.loads((nodes[index][1]/'status.json').read_text())
   wait_for(lambda:status(0).get('peer_name')=='Beta'and status(1).get('peer_name')=='Alpha')
   tone=subprocess.Popen(['pacat','--playback','--device=lan_mic_testtone','--rate=48000','--channels=1','--format=s16le','--latency-msec=40','--process-time-msec=10'],stdin=subprocess.PIPE);processes.append(tone)
   def write_tone():
    block=b''.join(struct.pack('<h',int(math.sin(i*440*math.tau/48000)*5000))for i in range(2400))
    try:
     while not stop.is_set():tone.stdin.write(block);tone.stdin.flush()
    except (OSError,ValueError):pass
   thread=threading.Thread(target=write_tone,daemon=True);thread.start();threads.append(thread)
   probe=subprocess.Popen(['parec','--record','--device=lan_mic_test_input','--rate=48000','--channels=1','--format=s16le','--latency-msec=40','--process-time-msec=10'],stdout=subprocess.PIPE)
   try:probe_audio=probe.communicate(timeout=.6)[0]
   except subprocess.TimeoutExpired:probe.terminate();probe_audio=probe.communicate(timeout=2)[0]
   probe_samples=array.array('h');probe_samples.frombytes(probe_audio[:len(probe_audio)//2*2]);assert len(probe_samples)>1000 and max(map(abs,probe_samples),default=0)>4000,'Synthetic microphone fixture is silent'
   recorder=subprocess.Popen(['pacat','--record','--device=lan_mic','--rate=48000','--channels=1','--format=s16le','--latency-msec=40','--process-time-msec=10'],stdout=subprocess.PIPE);processes.append(recorder)
   def read_recording():
    while data:=recorder.stdout.read(4096):received.extend(data)
   reader=threading.Thread(target=read_recording,daemon=True);reader.start();threads.append(reader)
   results=[]
   for sender,receiver in [(0,1),(1,0)]:
    log=nodes[sender][2]
    with log.open('a')as f:f.write('entering client 0 ...\nclient 0 acknowledged the connection!\n')
    wait_for(lambda:status(sender).get('capturing')and status(receiver).get('receiving'))
    mark=len(received);time.sleep(1.3)
    assert pulse('get-default-source')=='lan_mic';assert pulse('get-source-mute','lan_mic')=='Mute: no'
    with log.open('a')as f:f.write('releasing capture: synthetic return\n')
    wait_for(lambda:not status(sender).get('capturing')and not status(receiver).get('receiving'))
    assert pulse('get-default-source')==before;assert pulse('get-source-mute','lan_mic')=='Mute: yes'
    chunk=received[mark:];samples=array.array('h');samples.frombytes(chunk[:len(chunk)//2*2]);rms=math.sqrt(sum(s*s for s in samples)/max(1,len(samples)));assert len(samples)>24000 and rms>1000,(len(samples),rms)
    results.append({'direction':f'{["Alpha","Beta"][sender]} -> {["Alpha","Beta"][receiver]}','samples':len(samples),'rms':round(rms,1)})
   # Lose the transport during an active handoff; both endpoints must mute/restore.
   with nodes[0][2].open('a')as f:f.write('entering client 0 ...\nclient 0 acknowledged the connection!\n')
   wait_for(lambda:status(1).get('receiving'))
   for connection in connections:connection.shutdown(socket.SHUT_RDWR);connection.close()
   connections.clear();wait_for(lambda:pulse('get-default-source')==before and pulse('get-source-mute','lan_mic')=='Mute: yes')
   print(json.dumps({'linux_to_linux':results,'disconnect_restored_default':True,'speakers_preserved':pulse('get-default-sink')==sink_before,'topology':'two Linux agents on one host; real PipeWire audio; local duplex relay'}))
 finally:
  stop.set()
  for connection in connections:
   try:connection.shutdown(socket.SHUT_RDWR);connection.close()
   except OSError:pass
  for process in reversed(processes):
   if process.poll()is None:
    process.terminate()
    try:process.wait(timeout=8)
    except subprocess.TimeoutExpired:process.kill();process.wait()
  for thread in threads:thread.join(timeout=1)
  for process in processes:
   if process.stdin:
    try:process.stdin.close()
    except OSError:pass
  for module in reversed(modules):subprocess.run(['pactl','unload-module',module],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  pulse('set-default-source',before);pulse('set-default-sink',sink_before);pulse('set-source-mute','lan_mic','1')
  if was_active:subprocess.run(['systemctl','--user','start','lan-mic'],check=True)
if __name__=='__main__':main()
