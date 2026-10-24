import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { writeFile, mkdtemp, rm, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL("..", import.meta.url));
const require = createRequire(root + '/package.json');
const { build } = require('esbuild');
const directory = await realpath(await mkdtemp(join(tmpdir(), 'android-integration-')));
try {
  const mock = join(directory, 'hooks.mjs');
  const output = join(directory, 'checks.mjs');
  await writeFile(mock, `export let values = []; export let effects = []; let cursor = 0; let refCursor = 0; let effectCursor = 0; let refs = []; let slots = [];
export function dispose() { for (const slot of slots) slot?.cleanup?.(); slots = []; refs = []; effects = []; }
export function begin(seeds) { if (seeds) { dispose(); values = seeds; } cursor = 0; refCursor = 0; effectCursor = 0; effects.length = 0; }
export function useState(initial) { const index = cursor++; const state = values; if (!(index in state)) state[index] = typeof initial === 'function' ? initial() : initial; return [state[index], next => { state[index] = typeof next === 'function' ? next(state[index]) : next; }]; }
export function useEffect(action, dependencies) { const index = effectCursor++; const previous = slots[index]; if (!previous || !dependencies || dependencies.some((value, offset) => !Object.is(value, previous.dependencies[offset]))) effects.push(() => { previous?.cleanup?.(); slots[index] = {dependencies, cleanup: action()}; }); }
export function flushEffects() { for (const action of effects.splice(0)) action(); }
export function useRef(initial) { const index = refCursor++; return refs[index] ??= {current: initial}; }
export function useCallback(action) { return action; }
export function jsx(type, props, key) { return {type,props,key}; } export const jsxs = jsx; export const Fragment = Symbol.for("react.fragment");
`);
  const portal = join(directory, 'portal.mjs');
  await writeFile(portal, 'export function createPortal(children, container) { return {type:"portal",props:{children,container}}; }');
  const source = `
import assert from 'node:assert/strict';
import {begin, values, effects, flushEffects, dispose} from ${JSON.stringify(mock)};
import {Timeline} from ${JSON.stringify(root + '/src/Timeline.tsx')};
import {Capture} from ${JSON.stringify(root + '/src/Capture.tsx')};
import {Details} from ${JSON.stringify(root + '/src/Details.tsx')};
import {api} from ${JSON.stringify(root + '/src/api.ts')};
import {initialDraft} from ${JSON.stringify(root + '/src/state.ts')};
import {OperationStatus} from ${JSON.stringify(root + '/src/Operation.tsx')};
import {product} from ${JSON.stringify(root + '/src/product.ts')};
function all(node) { if (!node || typeof node !== 'object') return []; if (Array.isArray(node)) return node.flatMap(all); return [node, ...all(node.props?.children)]; }
const inspected = {version:1,id:'B',directory:'/evidence/B',device:null,started_at:'2026-10-23T00:00:00Z',legacy:false,metadata:null,warnings:[]};
const page = {generation:'g',rows:[],next_cursor:'older-again',total:401,matching:401,progress:{indexed:401,raw_bytes:100,indexed_bytes:100,backlog_bytes:0,complete:true,latest_sequence:401}};
const olderQuery = {source:'',search:'',cursor:'older-page-cursor',previous:[null],after:null,through:null,revision:0};
begin([{wrap:true,follow:false,order:'newest_first'},olderQuery,page,{kind:'closed'},'','',false]);
let tree = Timeline({inspected});
const followLabel = all(tree).find(node => node.type === 'label' && all(node).some(child => child.props?.type === 'checkbox') && JSON.stringify(node.props.children).includes('Follow latest'));
assert.ok(followLabel);
all(followLabel).find(node => node.props?.type === 'checkbox').props.onChange({target:{checked:true}});
assert.equal(values[0].follow,true);
assert.equal(values[1].cursor,null);
const calls = [];
product.query = async query => {calls.push(query); return page;};
begin();
tree = Timeline({inspected});
const originalTimer = globalThis.setTimeout;
globalThis.setTimeout = () => 0;
effects[3]();
await Promise.resolve();
await Promise.resolve();
dispose();
globalThis.setTimeout = originalTimer;
assert.equal(calls[0].cursor,null);
console.log('Follow latest resumes the current page after browsing older events.');
begin([false, ""]);
const reveals = [];
product.reveal = async (...arguments_) => {reveals.push(arguments_);};
let action;
const operation = {state:'finished',id:'save-A',result:{kind:'save',archive:'/evidence/A/capture-package.zip',directory:'/evidence/A'}};
const banner = OperationStatus({operation,change:()=>{},run:async (_label,callback)=>{action=callback();await action;},pending:'',recover:async()=>{}});
const archiveButton = all(banner).find(node=>node.type==='button');
archiveButton.props.onClick();
await action;
assert.deepEqual(reveals,[['archive','save-A']]);
console.log('Save success reveals its own archive while another session is inspected.');
function text(node) { if (Array.isArray(node)) return node.map(text).join(''); if (node && typeof node === 'object') return text(node.props?.children); return typeof node === 'string' || typeof node === 'number' ? String(node) : ''; }
function button(tree, label) { const result = all(tree).find(node => node.type === 'button' && text(node) === label); assert.ok(result, label); return result; }
function deferred() { let resolve, reject; const promise = new Promise((accept, fail) => {resolve=accept;reject=fail;}); return {promise,resolve,reject}; }
async function settle() { for (let count=0;count<6;count++) await Promise.resolve(); }
const event = id => ({id, timestamp:'2026-10-23T00:00:00Z',source:'network',kind:'http_request',summary:'GET https://example.invalid/'+id,raw_ref:'flows:1',clock_uncertainty_ms:null,original_timestamp:null,received_at:null});
const detail = id => ({event:event(id),detail_bytes:10,bodies:[]});
const eventPage = {...page,rows:[{sequence:1,event:event('one')},{sequence:2,event:event('two')}],next_cursor:null};
const requests = new Map();
product.detail = id => {const request=deferred();requests.set(id,request);return request.promise;};
product.query = async () => eventPage;
let focused = '';
const headingTarget = {focus:()=>{focused='heading';}};
const closeTarget = {focus:()=>{focused='close';}};
function renderTimeline(session=inspected, commit=true) {
  begin();
  const result = Timeline({inspected:session});
  for (const node of all(result)) {
    if (node.type === 'h2' && node.props.ref) node.props.ref.current=headingTarget;
    if (node.type === 'button' && node.props.ref) node.props.ref.current=closeTarget;
  }
  if (commit) flushEffects();
  return result;
}
function openEvent(tree, id, connected=true) {
  const origin={isConnected:connected,focus:()=>{focused=id;}};
  const row=all(tree).find(node=>node.props?.className?.startsWith('event ') && text(node).includes('/'+id));
  assert.ok(row);
  row.props.onClick({currentTarget:origin});
  return origin;
}
globalThis.setTimeout = () => 0;
try {
  begin([]);
  tree=renderTimeline();
  await settle();
  tree=renderTimeline();
  openEvent(tree,'one');
  tree=renderTimeline();
  assert.equal(all(tree).filter(node=>node.props?.role==='dialog').length,1);
  assert.match(text(tree),/Reading event/);
  assert.equal(focused,'close');
  const escaped=all(tree).find(node=>node.props?.role==='dialog');
  escaped.props.onKeyDown({key:'Escape',preventDefault:()=>{},stopPropagation:()=>{}});
  assert.equal(focused,'one');
  requests.get('one').resolve(detail('one'));
  await settle();
  tree=renderTimeline();
  assert.equal(all(tree).some(node=>node.props?.role==='dialog'),false);
  openEvent(tree,'one');
  tree=renderTimeline();
  const first=requests.get('one');
  openEvent(tree,'two',false);
  tree=renderTimeline();
  requests.get('two').resolve(detail('two'));
  await settle();
  first.resolve(detail('one'));
  await settle();
  tree=renderTimeline();
  const details=all(tree).find(node=>node.type===Details);
  assert.equal(details.props.detail.event.id,'two');
  assert.equal(details.key,JSON.stringify(['/evidence/B','two']));
  values[2]={...eventPage,rows:[]};
  tree=renderTimeline();
  assert.equal(all(tree).find(node=>node.type===Details).props.offPage,true);
  button(tree,'Close event details').props.onClick();
  assert.equal(focused,'heading');
  values[2]=eventPage;
  tree=renderTimeline();
  openEvent(tree,'one');
  tree=renderTimeline();
  const pending=requests.get('one');
  const otherSession={...inspected,id:'C',directory:'/evidence/C'};
  focused='unchanged';
  tree=renderTimeline(otherSession,false);
  assert.equal(all(tree).some(node=>node.props?.role==='dialog'),false);
  pending.resolve(detail('one'));
  await settle();
  assert.equal(values[3].kind,'loading');
  flushEffects();
  tree=renderTimeline(otherSession);
  assert.equal(values[3].kind,'closed');
  assert.equal(focused,'unchanged');
  await settle();
  tree=renderTimeline(otherSession);
  openEvent(tree,'one');
  tree=renderTimeline(otherSession);
  requests.get('one').reject(new Error('detail read failed'));
  await settle();
  tree=renderTimeline(otherSession);
  assert.match(text(all(tree).find(node=>node.props?.role==='alert')),/detail read failed/);
  button(tree,'Close event details').props.onClick();
  console.log('Inspector opens immediately, returns focus, preserves off-page selection, and rejects stale or cross-session details.');
  dispose();
  begin([]);
  const chunk=deferred();
  product.detailChunk=()=>chunk.promise;
  let detailTree=Details({detail:detail('body'),offPage:false,wrap:true});
  flushEffects();
  const abandoned=values;
  dispose();
  chunk.resolve({offset:0,total_bytes:3,data_base64:'YWJj',next_offset:null});
  await settle();
  assert.equal(abandoned[1],undefined);
  begin([]);
  const metadataCalls=[];
  const bodyCalls=[];
  product.detailChunk=async (...args)=>{metadataCalls.push(args);return {offset:0,total_bytes:0,data_base64:'',next_offset:null};};
  product.body=async (...args)=>{bodyCalls.push(args);return {chunk:{offset:args[2],total_bytes:65539,data_base64:'AP9B',next_offset:args[2]===0?65536:null}};};
  const bodyDetail={...detail('binary'),bodies:[{side:'response',available:true,bytes:65539,text:false,content_encoding:null,decoded_available:false,reason:null,decoded_reason:null}]};
  detailTree=Details({detail:bodyDetail,offPage:false,wrap:true});
  flushEffects();
  await settle();
  all(detailTree).find(node=>node.type==='button' && text(node).startsWith('Response')).props.onClick();
  begin();
  detailTree=Details({detail:bodyDetail,offPage:false,wrap:true});
  flushEffects();
  await settle();
  begin();
  detailTree=Details({detail:bodyDetail,offPage:false,wrap:true});
  assert.match(text(all(detailTree).find(node=>node.type==='pre')),/00000000  00 ff 41/);
  button(detailTree,'Next 64 KiB').props.onClick();
  begin();
  detailTree=Details({detail:bodyDetail,offPage:false,wrap:true});
  flushEffects();
  await settle();
  assert.deepEqual(bodyCalls,[['binary','response',0,false],['binary','response',65536,false]]);
  begin([]);
  detailTree=Details({detail:detail('replacement'),offPage:false,wrap:true});
  flushEffects();
  await settle();
  assert.deepEqual(metadataCalls.at(-1),['replacement',0]);
  assert.equal(values[0].kind,'metadata');
  assert.equal(values[2],0);
  begin([]);
  product.detailChunk=async()=>({offset:0,total_bytes:0,data_base64:'',next_offset:null});
  product.reveal=async()=>{throw new Error('reveal failed');};
  detailTree=Details({detail:detail('body'),offPage:false,wrap:true});
  flushEffects();
  button(detailTree,'Reveal raw source').props.onClick();
  await settle();
  begin();
  detailTree=Details({detail:detail('body'),offPage:false,wrap:true});
  assert.match(text(all(detailTree).find(node=>node.props?.role==='alert')),/reveal failed/);
  console.log('Details keeps binary paging, resets new mounts to metadata at byte zero, discards disposed chunks, and reports reveal errors locally.');
} finally {
  dispose();
  globalThis.setTimeout=originalTimer;
}
const limits={bytes:32000000,records:100000,duration_ms:120000};
const policy={max_flows:10000,max_body_bytes:33554432,max_header_bytes:131072,max_total_bytes:1073741824,duration_ms:3600000};
const device={serial:'phone-a',name:'Phone A',state:'ready'};
const idle={phase:'idle',session:{fixture:false,limits}};
const captureProps={snapshot:idle,operation:{state:'idle'},devices:[device],draft:{...initialDraft,serial:'phone-a'},change:()=>{},preferences:{output_root:'/capture',limits,network_policy:policy,adb_override:null,mitmdump_override:null},savePreferences:async()=>{},refresh:async()=>{},run:async(_label,callback)=>{action=callback();await action;},pending:'',onSnapshot:()=>{},onOperation:()=>{},setup:()=>{},actionHost:{}};
const starts=[];
api.start=async request=>{starts.push(request);return idle;};
function renderCapture(props=captureProps) {begin();return Capture(props);}
begin([]);
let capture=renderCapture();
flushEffects();
capture=renderCapture();
assert.equal(starts.length,0);
assert.equal(button(capture,'Start recording').props.disabled,false);
assert.match(text(capture),/whole-device HTTP[(]S[)] traffic.*proxy routing.*manual CA trust/);
assert.equal(all(capture).filter(node=>node.props?.type==='checkbox').length,0);
button(capture,'Start recording').props.onClick();
await action;
assert.equal(starts.length,1);
assert.deepEqual(starts[0].selection,{mode:'network',consent:{whole_device:true}});
assert.equal(starts[0].whole_device_consent,false);
const wholeDeviceProps={...captureProps,draft:{...captureProps.draft,selection:'logs_and_network',log_mode:'whole_device'}};
begin([]);
capture=renderCapture(wholeDeviceProps);
flushEffects();
capture=renderCapture(wholeDeviceProps);
assert.equal(button(capture,'Start recording').props.disabled,true);
button(capture,'Start recording').props.onClick();
await action;
assert.equal(starts.length,1);
all(capture).find(node=>node.props?.type==='checkbox').props.onChange({target:{checked:true}});
capture=renderCapture(wholeDeviceProps);
assert.equal(button(capture,'Start recording').props.disabled,false);
button(capture,'Start recording').props.onClick();
await action;
assert.equal(starts.length,2);
assert.equal(starts[1].whole_device_consent,true);
for (const props of [
  {...captureProps,snapshot:undefined},
  {...captureProps,preferences:undefined},
  {...captureProps,devices:[{...device,state:'unauthorized'}]},
  {...captureProps,pending:'Saving'},
  {...captureProps,operation:{state:'saving',id:'save',stage:'indexing',completed:0,total:null,cancel_allowed:true}},
  {...captureProps,snapshot:{phase:'recovery_required',session:{network:{state:'not_selected'},selection:{mode:'network'},warnings:[]}}},
  {...captureProps,draft:{...captureProps.draft,context:{title:'界'.repeat(171),notes:'',steps:[]}}},
]) {
  begin([]);
  capture=renderCapture(props);
  assert.equal(button(capture,'Start recording').props.disabled,true);
  button(capture,'Start recording').props.onClick();
  await action;
}
assert.equal(starts.length,2);
const stopCalls=[];
const saveCalls=[];
api.stop=async (...args)=>{stopCalls.push(args);return idle;};
api.save=async directory=>{saveCalls.push(directory);return {state:'saving',id:'save-active',stage:'indexing',completed:0,total:null,cancel_allowed:true};};
const activeProps={...captureProps,snapshot:{phase:'recording',session:{id:'active-A',directory:'/evidence/A',selection:{mode:'network',consent:{whole_device:true}},receipt:null,network:{state:'recording',detail:{decoded_http_events:1,tls_errors:0,admission_closed:false}},log_attribution:null,warnings:[]}}};
begin([]);
capture=renderCapture(activeProps);
assert.equal(button(capture,'Stop without archive').props.disabled,false);
assert.equal(button(capture,'Stop & Save').props.disabled,false);
button(capture,'Stop without archive').props.onClick();
await action;
assert.deepEqual(stopCalls,[[]]);
button(capture,'Stop & Save').props.onClick();
await action;
assert.deepEqual(saveCalls,[null]);
dispose();
console.log('Recording controls stop or save the active capture through their existing API callbacks.');
console.log('Explicit Start supplies per-recording network consent, while whole-device logs, context, pending work, and recovery remain gated.');

`;
  await build({stdin:{contents:source,resolveDir:root,sourcefile:'integration-check.tsx',loader:'tsx'},outfile:output,bundle:true,platform:'node',format:'esm',jsx:'automatic',plugins:[{name:'controlled-hooks',setup(builder){builder.onResolve({filter:/^react(?:\/jsx-runtime)?$/},()=>({path:mock}));builder.onResolve({filter:/^react-dom$/},()=>({path:portal}));}}]});
  await import(pathToFileURL(output).href);
} finally {
  await rm(directory,{recursive:true,force:true});
}
