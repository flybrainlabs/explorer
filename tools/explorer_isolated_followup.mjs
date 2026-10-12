// A real isolated-bank followup; never accepts a non-loopback endpoint.
import {Connection,PublicKey,Keypair,Transaction,SystemProgram,sendAndConfirmTransaction} from '@solana/web3.js';
import {getAssociatedTokenAddressSync,TOKEN_PROGRAM_ID} from '@solana/spl-token';
import {readFile,writeFile} from 'node:fs/promises';
import {addresses,decodeSlot,meta,instruction,Writer} from './economy_v2_client.mjs';
import {hash,text} from '../web/campaign-codec.mjs';
const root='data/prepared/explorer-20261012',config=JSON.parse(await readFile(root+'/isolated-chain-config.json','utf8'));
if(!/^http:\/\/127\.0\.0\.1:\d+$/.test(config.rpc))throw new Error('Isolated loopback endpoint required');
const c=new Connection(config.rpc,'finalized'),pid=new PublicKey(config.programId),a=addresses(pid),payer=Keypair.fromSecretKey(Uint8Array.from(JSON.parse(await readFile('/workspace/.flypow-tools/explorer-isolated-authority-20261012.json','utf8')))),previous=JSON.parse(await readFile(root+'/multi-payment.json','utf8'));
const payment=hash(text('explorer-followup-'+Date.now())),ix=[];for(const value of previous.slots){const right=new PublicKey(value),s=decodeSlot((await c.getAccountInfo(right)).data),beneficiary=new PublicKey(s.beneficiary),ata=getAssociatedTokenAddressSync(a.mint,beneficiary);ix.push(instruction(pid,[meta(a.global),meta(new PublicKey(s.stage)),meta(right,true),meta(a.pool(1),true),meta(ata,true),meta(a.payment(right,payment),true),meta(payer.publicKey,true,true),meta(SystemProgram.programId),meta(TOKEN_PROGRAM_ID)],5,new Writer().key(payment).u64(3n).result()));}
const signature=await sendAndConfirmTransaction(c,new Transaction().add(...ix),[payer],{commitment:'finalized'});
config.reviewedSignatures=[signature];await writeFile(root+'/isolated-chain-config.json',JSON.stringify(config,null,2)+'\n');await writeFile(root+'/multi-payment-followup.json',JSON.stringify({signature,slots:previous.slots,paymentId:Buffer.from(payment).toString('hex'),amountAtomsPerSlot:'3',environment:'isolated-loopback-bank',formalFunds:false},null,2)+'\n');console.log({realIsolatedClaims:2,formalFunds:false});
