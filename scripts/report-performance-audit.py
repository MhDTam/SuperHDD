import json
import re
import statistics
from pathlib import Path

root = Path(__file__).resolve().parent.parent / 'artifacts/audit-performance'
read = lambda name: json.loads((root / name).read_text(encoding='utf-8-sig'))
pool = read('pool-comparison-final.json')
source = read('source-comparison-final.json')
release = read('release-proof.json')
preservation = read('game-preservation-proof.json')
ui = read('ui-tests.json')['stats']
assert ui['unexpected'] == 0 and ui['expected'] == 40
rust_log = (root / 'rust-tests.log').read_text(encoding='utf-8-sig')
rust = sum(map(int, re.findall(r'test result: ok\. (\d+) passed;', rust_log)))
assert rust == 124 and 'test result: FAILED' not in rust_log
assert release['workerSha256'] == pool['workerSha256'] == source['candidateSha256']
assert source['oldSha256'] != source['candidateSha256']
assert preservation['fileMetadataUnchanged'] and preservation['exeSha256Unchanged']
checks = {}
for name in ['filesystem-proof.json', 'session-proof.json', 'manager-proof.json']:
    proof = read(name)
    assert proof['passed'] and proof['workerSha256'] == release['workerSha256']
    checks[name] = sum(value is True for value in proof['checks'].values())

comparisons = []
for threads in [1, 8]:
    for size in [4096, 65536]:
        rates = {engine: statistics.mean(case['verifiedMiBPerSecond']
                 for run in pool['rounds'] if run['engine'] == engine
                 for case in run['measurement']['cases'] if case['threads'] == threads and case['requestBytes'] == size)
                 for engine in ['baseline', 'candidate']}
        comparisons.append({'threads': threads, 'requestBytes': size, **rates, 'gainPercent': 100 * (rates['candidate']/rates['baseline'] - 1)})
summaries = {engine: statistics.mean(run['measurement']['summaryAverageUs'] for run in pool['rounds'] if run['engine'] == engine)
             for engine in ['baseline', 'candidate']}
summary_speedup = summaries['baseline']/summaries['candidate']
report = {'completedAt': release['timestamp'], 'release': release,
          'rustTestsPassed': rust, 'uiTestsPassed': ui['expected'], 'integrationChecksPassed': checks,
          'clippy': 'workspace/all targets, warnings denied, passed', 'warmPoolComparisons': comparisons,
          'snapshotAverageMicroseconds': summaries, 'snapshotSpeedup': summary_speedup,
          'sourceComparisonRunId': source['runId'], 'gamePreservation': preservation,
          'scope': 'Current selected-folder cache/backend, native manager, telemetry and desktop. No game FPS or loading speedup established by this audit.'}
(root/'summary.json').write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n', encoding='utf-8')

rtl = lambda text: '\u2067'+text+'\u2069'
ltr = lambda text: '\u2066'+text+'\u2069'
lines = [
    '# '+rtl('تقرير فحص Super HDD وتحسين الأداء — 23 أكتوبر 2026'), '',
    rtl('اكتمل فحص المسار الحالي للكاش على هذه الحاسبة. النسخة المحسّنة مفتوحة، وكاش Euro Truck فعّال مجددًا بالسعة الأصلية 25 GB. لم نغيّر إعدادات الإقلاع أو ملف الذاكرة الافتراضية أو تعريف Dokan المثبّت.'), '',
    '## '+rtl('التحسينات المعتمدة'), '',
    '- '+rtl('فصل قراءة SSD والتحقق ببصمة SHA-256 عن القفل العام، مع رفض أي قراءة ممزوجة عند إخلاء البلوك أو إعادة استخدام مكانه.'),
    '- '+rtl('تحديث حساب الاستخدام حسب الملفات بدل مسح جميع البلوكات كل نصف ثانية. التحديث الدوري يتجاوز انشغال القفل ويعيد المحاولة في الدورة التالية.'),
    '- '+rtl('تفعيل تحسينات المترجم /O2 لعامل الكاش، ونقل جمع بيانات الأداء الأصلية إلى مهمة خلفية بدل تعطيل الواجهة.'),
    '- '+rtl('تصحيح نسبة الإصابة لتشمل عداداتها القراءات التي تتجاوز الكاش. بقيت الكتابة المتزامنة للملفات الأصلية والتحقق من سلامة البيانات كما هما.'), '',
    '## '+rtl('نتائج المقارنة الداخلية'), '',
    rtl('النتائج التالية متوسط ثلاث جولات لكل نسخة. اختبار الكاش يحتوي 2048 بلوكًا مقيمًا، وخيط قراءة واحد أو ثمانية. عند فقد الكاش ينتظر الاختبار 1 ms بدل قراءة HDD فعلية؛ لذلك الأرقام لا تمثّل سرعة اللعبة أو الهارد نفسه.'), '',
]
for c in comparisons:
    label = 'قراءة متوازية، 8 خيوط' if c['threads'] == 8 else 'قراءة بخيط واحد'
    lines += ['- '+rtl(f"{label}، {ltr(str(c['requestBytes']//1024)+' KiB')}: {ltr(f'{c['baseline']:.1f} → {c['candidate']:.1f} MiB/s')}، الفرق {ltr(f'{c['gainPercent']:+.1f}%')}.")]
lines += ['', rtl(f"متوسط تحديث العدادات: {ltr(f'{summaries['baseline']:.2f} → {summaries['candidate']:.2f} µs')}؛ أسرع بنحو {ltr(f'{summary_speedup:.0f}×')} في تجربة الملف الواحد."), '',
    rtl('تحسّن التوازي واضح في هذه التجربة؛ الأداء بخيط واحد لم يتغير كثيرًا، وانخفضت قراءة 4 KiB المنفردة بنحو 3%. فحص مسار المصدر المنفصل يستخدم ملفًا مدفأً في كاش ويندوز، ولا يثبت تحسن زمن بحث الهارد الفعلي.'), '',
    '## '+rtl('الفحوص التي نجحت'), '',
    '- '+rtl(f'{rust} اختبار Rust و{ui["expected"]} اختبار واجهة، وفحص Clippy لكل أهداف المشروع بلا تحذيرات.'),
    '- '+rtl(f'{checks["filesystem-proof.json"]} فحصًا للملفات والكتابة والـmapped writes والـstreams والاستعادة، و{checks["session-proof.json"]} فحصًا للكاش المشترك والأقراص المستقلة، و{checks["manager-proof.json"]} فحصًا لإدارة الإعدادات.'),
    '- '+rtl('فحوص C++ للملكية والسعة المشتركة وLRU والقراءات الجزئية والبصمات والتزامن، مع إجبار إعادة استخدام بلوك وتعطل قفل أثناء وجود قراءة معلّقة.'),
    '- '+rtl('تطابقت المسارات والأحجام وتواريخ التعديل للملفات الأصلية الـ132. تطابقت أيضًا بصمة ملف تشغيل اللعبة بعد قراءته عبر العامل الجديد، وبقيت الإعدادات وهويات الأقراص وملفات الألعاب المسجّلة محفوظة.'), '',
    '## '+rtl('ما لم يثبته هذا الفحص'), '',
    rtl('لم ننفّذ مقارنة جديدة داخل اللعبة لنفس الحفظ والطريق أو شاشة التحميل. لذلك لا نحوّل نسب التحسن الداخلية إلى زيادة FPS، ولا نؤكد اختفاء كل تقطيع. التجربة السابقة بلا كاش تبقى مرجعًا غير مضبوط المسار.'), '',
    rtl('وضع أولوية التحميل ما زال تقريبيًا؛ يعتمد إعادة القراءة وكثافة النشاط، وقد يقبل بعض بيانات اللعب أو يتجاوز بيانات تحميل تُقرأ مرة واحدة. بقي الكاش بسعة مشتركة واحدة، والنسخ المخزّنة قابلة للحذف؛ ليست الملفات الأصلية.'), '',
    rtl('تغيّر تنسيق فهرس الكاش، ولذلك أُعيد تكوين النسخ المؤقتة أثناء التحديث. يحتاج الكاش إلى تجميع البيانات المفيدة مجددًا. لا توجد طبقة RAM مخصصة لبيانات الكاش، مع بقاء كاش ويندوز وذاكرة الفهرس والمخازن المؤقتة المعتادة.'), '',
    rtl('قراءة صغيرة من الكاش قد تظل تتحقق من مدى يصل إلى 64 KiB. أضيف عداد cacheReadApiBytes لرصد هذه الكلفة. الاستعادة بعد إعادة تشغيل ويندوز تظل مرتبطة بالسجل؛ لم نضف تشغيلًا تلقائيًا جديدًا إلى ويندوز.'), '',
    '## '+rtl('ملفات الأدلة'), '',
]
for name in ['summary.json','pool-comparison-final.json','source-comparison-final.json','release-proof.json','game-preservation-proof.json','rust-tests.log','clippy.log','ui-tests.json']:
    lines.append(f'- [{name}](<{(root/name).as_posix()}>)')
(root/'report-ar.md').write_text('\n'.join(lines)+'\n', encoding='utf-8')
print(json.dumps({'rust':rust,'ui':ui['expected'],'integration':checks,'snapshotSpeedup':summary_speedup,'parallelGains':[c for c in comparisons if c['threads']==8]},indent=2))
