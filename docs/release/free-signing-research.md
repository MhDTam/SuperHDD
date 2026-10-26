# Super HDD: free-only signing research — 2026-10-19

⁧طلب المستخدم حلاً مجانياً. ميزانية التوقيع صفر، ولا توجد شهادة EV أو عضوية Microsoft Hardware Developer متاحة. البحث أدناه لا يثبت وجود مسار مجاني مكتمل لتوقيع تعريف النواة الخاص بنا، ولا يغيّر حالة التطبيق إلى جاهز أو مفعّل.⁩

## النتيجة الموثقة

⁧تقدم [SignPath Foundation](https://signpath.org/about) توقيعاً مجانياً للمشاريع المفتوحة المصدر التي تقبلها. تشترط [سياساتها](https://signpath.org/terms) ترخيصاً معتمداً من OSI، ومصدراً مفتوحاً للمكونات، ومشروعاً منشوراً وموثقاً ومستمراً، وأصل بناء قابلاً للتحقق. القبول تقديري، وتصدر الشهادة باسم المؤسسة. هذا عرض لتوقيع البرامج؛ لم أجد في الصفحات الرسمية التي راجعتها تعهداً بتقديم تعريف تخزين نواة جديد إلى Microsoft عبر حساب المؤسسة. وجود مشروع عرض افتراضي أو برنامج لإدارة التعريفات ضمن المشاريع المدعومة لا يثبت تلك الخدمة.⁩

⁧تطلب [Microsoft](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/code-signing-reqs) شهادة EV سارية مرتبطة بحساب لوحة العتاد عند تقديم التعريف. لذلك توقيع ملف EXE أو CAB وحده لا يثبت أننا حصلنا على توقيع Microsoft للملف SYS أو أصبحنا مؤهلين لتقديمه. كذلك تميز [وثائق Artifact Signing](https://learn.microsoft.com/en-us/azure/artifact-signing/faq) توقيع تطبيقات المستخدم عن توقيع النواة عبر Partner Center؛ هذه الخدمة لا تصدر EV.⁩

⁧يوفر [Microsoft Store](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options) توقيعاً مجانياً لحزمة تطبيق MSIX المقبولة. هذا لا يثبت اعتماد تعريف التخزين المرفق. أما [توقيع attestation](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/driver-signing-offerings) فله نطاق اختباري، ويتطلب حساب التقديم المؤهل، ولا يثبت اجتياز HLK أو سلامة وظيفة التعريف.⁩

⁧الاحتمال المجاني للمستخدم هو قبول جهة راعية تحمل مسؤولية النشر وتقديم تعريفنا إلى Microsoft على نفقتها. هذا احتمال يحتاج تأكيداً مباشراً من الجهة؛ لم يُحصل على قبول أو التزام، ولا يُعد SignPath راعياً مؤكداً لتعريفنا. لا تُقترح شهادة مدفوعة كحل ضمن الميزانية الحالية.⁩

## استفسار جاهز للمراجعة، لم يُرسل

Subject: Free OSS sponsorship eligibility for a new Windows storage-filter kernel driver

We are developing Super HDD, a Windows x64 desktop application and our own
storage-filter kernel driver. The project is currently local and has not been
published as an open-source release. No decision to publish or adopt an OSS
license has been made. We have no EV certificate or Microsoft Hardware Developer
account, and the signing budget is zero.

Before considering an application to your OSS program, could you clarify:

1. Does your free Foundation program cover a newly developed kernel-mode `.sys`
   storage filter, or only application/installer Authenticode signing?
2. If kernel drivers are supported, would your organization perform the Microsoft
   Hardware Developer submission and obtain the returned Microsoft-signed driver
   package using its own eligible publisher account? Please distinguish this from
   signing a CAB file that we would still need our own account to submit.
3. Would a supported project need its own EV certificate, publisher registration,
   paid signing/HLK services, or any other mandatory expenditure?
4. What public-release, license, reputation, review and reproducible-build
   requirements must a new project meet before it can be considered?

The driver is still experimental. Scoped test-signed VM correctness, benchmark
and stress checks have passed. Physical-source support, lifecycle integration,
recovery, production qualification and signed installation remain incomplete.
We are seeking eligibility information, not asking you to sign an unqualified
binary or asserting that the current driver is production-ready.

## الحالة الفعلية

⁧لم يُرسل هذا الاستفسار، ولم يُرفع كود أو ملف تنفيذي، ولم تتغير رخصة المشروع أو خصوصيته. لم تُنشأ حسابات ولم تُقبل اتفاقيات ولم تُشترَ خدمة. التطوير والاختبار داخل المختبر يظلان ممكنين بالأدوات المتوفرة، لكنهما لا يثبتان صلاحية تشغيل التعريف على أقراص المستخدم. لم تتغير حماية ويندوز، والتسريع الشامل ما زال غير مفعّل؛ اكتمال المحرك وربطه وتأهيله متطلبات مستقلة عن التوقيع.⁩
