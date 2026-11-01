import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { cpus, totalmem } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const executable = path.resolve(process.argv[2] || path.join(root, 'target/release/framejet'));
const directory = path.resolve(process.argv[3] || path.join(root, 'artifacts/svg-speed'));
mkdirSync(directory);
const hash = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const cases = [
  { name:'css-smil-540p', input:'examples/css-smil.svg', width:960, height:540 },
  { name:'css-smil-1080p', input:'examples/css-smil.svg', width:1920, height:1080 },
  { name:'matched-native', input:'bench/animated-native.svg.mjs', width:960, height:540 },
  { name:'matched-browser', input:'tests/fixtures/animated.svg', width:960, height:540 },
];
const manifest = {
  version: spawnSync(executable, ['--version'], {encoding:'utf8'}).stdout.trim(),
  binary_sha256:hash(executable), cpu:cpus()[0]?.model, logical_cpus:cpus().length,
  memory_bytes:totalmem(), encoder:'libx264', crf:18, preset:'fast', fps:30, duration:4, frames:120,
  warmups:1, repetitions:3,
  ffmpeg:spawnSync('ffmpeg',['-version'],{encoding:'utf8'}).stdout.split('\n')[0],
  cases:cases.map(fixture=>({...fixture,sha256:hash(path.join(root,fixture.input))})),
};
const records=[];
const median=values=>[...values].sort((first,second)=>first-second)[Math.floor(values.length/2)];
for(let round=-1;round<3;round++){
  for(const fixture of round%2===0?[...cases].reverse():cases){
    const output=path.join(directory,`${fixture.name}.mp4`);
    const started=process.hrtime.bigint();
    const result=spawnSync(executable,['render',path.join(root,fixture.input),'-o',output,'--force',
      '--width',String(fixture.width),'--height',String(fixture.height),'--fps','30','--duration','4',
      '--encoder','software','--crf','18'],{encoding:'utf8',timeout:120000,maxBuffer:4*1024*1024});
    const wall=Number(process.hrtime.bigint()-started)/1e9;
    assert.equal(result.status,0,result.error||result.stderr);
    const report=JSON.parse(result.stdout);
    assert.equal(report.frames,120);assert.equal(report.encoder,'libx264');
    assert.equal(report.output_width,fixture.width);assert.equal(report.output_height,fixture.height);
    records.push({name:fixture.name,warmup:round<0,round,wall_seconds:wall,processing_seconds:report.processing_seconds});
    writeFileSync(path.join(directory,'results.json'),JSON.stringify({manifest,records},null,2)+'\n');
    console.log(`${round<0?'warmup':`round ${round+1}`} ${fixture.name}: ${wall.toFixed(3)}s`);
  }
}
const summary=cases.map(fixture=>{
  const samples=records.filter(record=>record.name===fixture.name&&!record.warmup).map(record=>record.wall_seconds);
  const seconds=median(samples);
  return {name:fixture.name,width:fixture.width,height:fixture.height,median_seconds:seconds,whole_command_fps:120/seconds,samples};
});
for(const fixture of cases){
  const output=path.join(directory,`${fixture.name}.mp4`);
  const result=spawnSync('ffprobe',['-v','error','-select_streams','v:0','-count_frames','-show_entries',
    'stream=codec_name,width,height,nb_read_frames,r_frame_rate,duration','-of','json',output],{encoding:'utf8'});
  assert.equal(result.status,0,result.stderr);
  const stream=JSON.parse(result.stdout).streams[0];
  assert.equal(stream.codec_name,'h264');assert.equal(Number(stream.nb_read_frames),120);
  assert.equal(Number(stream.duration),4);assert.equal(stream.r_frame_rate,'30/1');
  assert.equal(stream.width,fixture.width);assert.equal(stream.height,fixture.height);
}
writeFileSync(path.join(directory,'results.json'),JSON.stringify({manifest,records,summary},null,2)+'\n');
console.log(JSON.stringify(summary,null,2));
