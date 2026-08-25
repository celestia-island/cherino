# الأمان

يفرض Cherino الدفاع المتعدد الطبقات للحاويات المعزولة عبر طبقة
مشتركة من ملفات الأمان. أنواع السياسة نفسها تستهلكها كل واجهة خلفية،
بحيث يفرض وقتَا تشغيل Docker وYouki القيود ذاتها.

## ملفات الأمان

يجمع `ContainerSecurity` سياسة كل عبء عمل:

- `cap_drop` / `cap_add` — مجموعات قدرات Linux؛
- `security_opt` — خيارات أمان Docker (مثل `no-new-privileges:true`)؛
- `egress_policy` — سياسة الـ egress الشبكية (انظر أدناه).

توجد ملفات جاهزة في `cherino::security_profile`: `postgres()` و
`scepter()` و`scepter_readonly()` و`cosmos()`. وهي تُرمّز الإعدادات
الافتراضية المُحصّنة للمنصة — فمثلًا، يسقط `scepter()` جميع القدرات
`ALL` ويعيد إضافة الحد الأدنى فقط الذي تحتاجه حاوياته الفرعية.

مع ميزة `docker`، يطبّق `cherino::apply_to_host_config` سياسة
`ContainerSecurity` على `HostConfig` من Bollard، مترجمًا السياسة إلى
إعدادات Docker الأصلية (بما فيها قواعد الـ egress).

## seccomp

يصف `SeccompProfile` / `SeccompProfileData` ملفات ترشيح استدعاءات
النظام، ويُصيّر `build_security_opts` الملف إلى مدخلات `security_opt`
لإعداد الحاوية. ترشيح seccomp هو خط الدفاع الأول: فهو يقيّد أي
استدعاءات النظام يمكن لعبء العمل المعزول أن يجريها أصلًا.

## AppArmor

تحتاج الحاويات المتداخلة إلى وصل أنظمة ملفات FUSE، وهو ما تحجبه
سياسة AppArmor الافتراضية. يشحن Cherino ملفًّا مخصصًا،
`celestia-cherino-fuse` (`crates/cherino/src/apparmor/celestia-cherino-fuse`)،
يُثبَّت على المضيف مرة واحدة، بصلاحيات الجذر:

```console
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

عند وقت التشغيل، يكتشف cherino أي ملف مثبّت
(`installed_profile_name`) ويلحقه عبر `fuse_security_opts`. المضيفون
الذين ما زالوا يحملون ملف `celestia-plana-fuse` القديم يُقبلون مع
تحذير بالإهمال.

مخرج الطوارئ `CHERINO_APPARMOR_UNCONFINED` (القديم:
`PLANA_APPARMOR_UNCONFINED`) يتخطى تقييد AppArmor للمضيفين الذين
لا يمكن تثبيت أي ملف لديهم — وكل استخدام يصدر `tracing::warn!`.

## Landlock

يعبّر `LandlockRules` عن قواعد الوصول إلى نظام الملفات المفروضة عبر
Landlock في Linux، مقيّدًا أي المسارات يمكن للعملية المعزولة لمسها
باستقلال عن حدود الحاوية.

## التحكم في الـ egress

يتحكم `EgressPolicy` في الوصول الشبكي الصادر بثلاثة أنماط
(`EgressMode`): `DenyAll` (الافتراضي) و`AllowAll` و`Whitelist`.
وتُبنى السياسات بأسلوب انسيابي (fluent):

```rust
use cherino::EgressPolicy;

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .allow_network("192.0.2.0/24")
    .with_dns_server("192.0.2.53");
```

يحجب `EgressPolicy::deny_all()` كل الـ egress؛ ويعيد
`entelecheia_default()` قائمة السماح القياسية للمنصة. على Docker،
تتحقق السياسة عبر تثبيت DNS ومدخلات `extra_hosts` بحيث لا تُترجم
إلى عناوين حقيقية إلا الأسماء المدرجة في قائمة السماح.

## قائمة السماح بالسجلّات

يقيّد `RegistryWhitelist` (مع `RegistryEntry`) السجلّات التي يجوز سحب
صور الحاويات منها. يمكن تحميل قوائم السماح من ملف
(`RegistryWhitelist::load`)، أو تحليلها من نص
(`RegistryWhitelist::parse`)، أو حلّها من مساحة عمل
(`resolve_from_workspace`).

## التشغيل دون امتيازات الجذر

تشغّل واجهة `cherino-runtime` (Youki/libcontainer) الخلفية الحاويات
دون امتيازات الجذر ودون خادم خلفي: لا يحوز أي خادم خلفي ذو امتيازات
حالة الحاويات، وتُنفَّذ أعباء العمل المعزولة بصلاحيات المستخدم
المُستدعي — ما يقلّص سطح الهجوم ذي الامتيازات على المضيفين الذين
تُستخدم عليهم.

## العلامة والتوافقية

استُخرج `cherino` من مساحة عمل `plana`. الأسماء القديمة ما تزال
تُقرأ للتوافقية، مع إصدار `tracing::warn!` لكل منها:

| القديم (plana / entelecheia) | الجديد (cherino) |
|------------------------------|---------------|
| ملف AppArmor `celestia-plana-fuse` | `celestia-cherino-fuse` |
| متغير البيئة `PLANA_APPARMOR_UNCONFINED` | متغير البيئة `CHERINO_APPARMOR_UNCONFINED` |
| متغير البيئة `ENTELECHEIA_RUN_DIR` | متغير البيئة `CHERINO_RUN_DIR` |
| مجلد التشغيل `/tmp/entelecheia/youki` | مجلد التشغيل `/tmp/cherino/youki` |

تحتفظ التجاوزات العامة (`CONTAINER_RUN_DIR` و`CONTAINER_ROOTFS_URL`
و`CONTAINER_NETWORK`) بأسبقيتها على جميع الأسماء ذات العلامة.
