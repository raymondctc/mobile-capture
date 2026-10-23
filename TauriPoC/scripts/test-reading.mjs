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
  await writeFile(mock, `export let values = []; export let effects = []; let cursor = 0;
export function begin(seeds) { if (seeds) values = seeds; cursor = 0; effects.length = 0; }
export function useState(initial) { const index = cursor++; if (!(index in values)) values[index] = typeof initial === 'function' ? initial() : initial; return [values[index], next => { values[index] = typeof next === 'function' ? next(values[index]) : next; }]; }
export function useEffect(action) { effects.push(action); }
export function useRef(initial) { return {current: initial}; }
export function useCallback(action) { return action; }
export function jsx(type, props) { return {type,props}; } export const jsxs = jsx; export const Fragment = Symbol.for("react.fragment");
`);
  const source = `
import assert from 'node:assert/strict';
import {begin, values, effects} from ${JSON.stringify(mock)};
import {Timeline} from ${JSON.stringify(root + '/src/Timeline.tsx')};
import {OperationStatus} from ${JSON.stringify(root + '/src/Operation.tsx')};
import {product} from ${JSON.stringify(root + '/src/product.ts')};
function all(node) { if (!node || typeof node !== 'object') return []; if (Array.isArray(node)) return node.flatMap(all); return [node, ...all(node.props?.children)]; }
const inspected = {version:1,id:'B',directory:'/evidence/B',device:null,started_at:'2026-10-23T00:00:00Z',legacy:false,metadata:null,warnings:[]};
const page = {generation:'g',rows:[],next_cursor:'older-again',total:401,matching:401,progress:{indexed:401,raw_bytes:100,indexed_bytes:100,backlog_bytes:0,complete:true,latest_sequence:401}};
const olderQuery = {source:'',search:'',cursor:'older-page-cursor',previous:[null],after:null,through:null,revision:0};
begin([{wrap:true,follow:false,order:'newest_first'},olderQuery,page,undefined,false,'','',false]);
let tree = Timeline({inspected,run:async (_label,action) => action()});
const followLabel = all(tree).find(node => node.type === 'label' && all(node).some(child => child.props?.type === 'checkbox') && JSON.stringify(node.props.children).includes('Follow latest'));
assert.ok(followLabel);
all(followLabel).find(node => node.props?.type === 'checkbox').props.onChange({target:{checked:true}});
assert.equal(values[0].follow,true);
assert.equal(values[1].cursor,null);
const calls = [];
product.query = async query => {calls.push(query); return page;};
begin();
tree = Timeline({inspected,run:async (_label,action) => action()});
const originalTimer = globalThis.setTimeout;
globalThis.setTimeout = () => 0;
const cleanup = effects[2]();
await Promise.resolve();
await Promise.resolve();
cleanup();
globalThis.setTimeout = originalTimer;
assert.equal(calls[0].cursor,null);
console.log('Follow latest resumes the current page after browsing older events.');
begin([false, ""]);
const reveals = [];
product.reveal = async (...arguments_) => {reveals.push(arguments_);};
let action;
const operation = {state:'finished',id:'save-A',result:{kind:'save',archive:'/evidence/A/capture-package.zip',directory:'/evidence/A'}};
const banner = OperationStatus({operation,change:()=>{},run:async (_label,callback)=>{action=callback();await action;},pending:'',recover:async()=>{}});
const button = all(banner).find(node=>node.type==='button');
button.props.onClick();
await action;
assert.deepEqual(reveals,[['archive','save-A']]);
console.log('Save success reveals its own archive while another session is inspected.');
`;
  await build({stdin:{contents:source,resolveDir:root,sourcefile:'integration-check.tsx',loader:'tsx'},outfile:output,bundle:true,platform:'node',format:'esm',jsx:'automatic',plugins:[{name:'controlled-hooks',setup(builder){builder.onResolve({filter:/^react(?:\/jsx-runtime)?$/},()=>({path:mock}));}}]});
  await import(pathToFileURL(output).href);
} finally {
  await rm(directory,{recursive:true,force:true});
}
