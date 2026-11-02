const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const line=source.split('\n').find(item=>item.startsWith('function listingObservationText('));
assert.ok(line);
const text=vm.runInNewContext(`(${line})`);
assert.match(text({status:'recent',age_seconds:10}),/less than an hour ago.*availability unverified/);
assert.match(text({status:'old',age_seconds:259201}),/3 days ago.*refresh recommended.*availability unverified/);
for(const value of [null,{}, {status:'recent',age_seconds:-1}, {status:'old',age_seconds:NaN}])assert.match(text(value),/time unknown/);
const find=name=>source.split('\n').find(item=>item.startsWith(`function ${name}(`));
const filter={checked:true},query={value:''},review={checked:false};
const context={document:{querySelector:s=>({'#filter':query,'#review-only':review,'#observation-filter':filter})[s]},state:{reviews:[],jobs:[
 {job:{id:'recent',title:'Engineer',employer:'Example',location:''},observation:{status:'recent',age_seconds:1}},
 {job:{id:'old',title:'Engineer',employer:'Example',location:''},observation:{status:'old',age_seconds:300000}},
 {job:{id:'unknown',title:'Engineer',employer:'Example',location:''},observation:{status:'unknown'}},
 {job:{id:'bad',title:'Engineer',employer:'Example',location:''},observation:{status:'recent',age_seconds:-1}}
]}};
vm.runInNewContext(`${find('jobObservationNeedsReview')}\n${find('filteredJobRows')}\nthis.filtered=filteredJobRows`,context);
assert.equal(context.filtered().map(row=>row.job.id).join(','),'old,unknown,bad');
filter.checked=false;assert.equal(context.filtered().length,4);
const sourceBoard=vm.runInNewContext(`(${find('sourceBoardForRefresh')})`,{URL});
for(const [source,url,board] of [
 ['greenhouse','https://job-boards.eu.greenhouse.io/unframe/jobs/5010248101','unframe'],
 ['lever','https://jobs.lever.co/example/fictional','example'],
 ['ashby','https://jobs.ashbyhq.com/example/fictional','example']
])assert.equal(sourceBoard({source,canonical_url:url}).board,board);
for(const job of [
 {source:'greenhouse',canonical_url:'https://job-boards.greenhouse.io.evil.test/example/jobs/1'},
 {source:'greenhouse',canonical_url:'https://job-boards.greenhouse.io/example'},
 {source:'lever',canonical_url:'https://user@jobs.lever.co/example/role'},
 {source:'ashby',canonical_url:'https://jobs.ashbyhq.com/a%20b/role'},
 {source:'user_supplied_url',canonical_url:'https://example.org/role'}
])assert.equal(sourceBoard(job),null);
const resultText=vm.runInNewContext(`(${find('listingBoardCheckText')})`);
assert.match(resultText({status:'found'}),/exact saved listing URL.*availability/i);
assert.match(resultText({status:'not_seen'}),/does not establish closure/i);
assert.match(resultText({status:'unavailable'}),/could not be checked/i);
console.log('Listing observation UI checks passed.');
