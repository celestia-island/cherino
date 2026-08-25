<h1 align="center">Cherino</h1>

<p align="center"><strong>عدة موحّدة لعمليات الحاويات، مستقلة عن وقت التشغيل، لمنصة celestia</strong></p>

<div align="center">

[![License: SySL-1.0](https://img.shields.io/badge/License-SySL--1.0-blue.svg)](https://sysl.celestia.world)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fcherino-blue.svg)](https://github.com/celestia-island/cherino)
[![Docs](https://img.shields.io/badge/docs-cherino.docs.celestia.world-blue)](https://cherino.docs.celestia.world)
[![docs.rs](https://docs.rs/cherino/badge.svg)](https://docs.rs/cherino)
[![Checks](https://img.shields.io/github/actions/workflow/status/celestia-island/cherino/checks.yml)](https://github.com/celestia-island/cherino/actions/workflows/checks.yml)

</div>

<div align="center">

[English](../../README.md) · [简体中文](../zhs/README.md) · [繁體中文](../zht/README.md) · [日本語](../ja/README.md) · [한국어](../ko/README.md) · [Français](../fr/README.md) · [Español](../es/README.md) · [Русский](../ru/README.md) · **العربية**

</div>

Cherino هي عدة لعمليات الحاويات خاصة بمنصة celestia — مكتبة
[Rust](https://www.rust-lang.org/) مستقلة توفّر واجهة برمجية موحّدة
ومستقلة عن وقت التشغيل لإنشاء الحاويات المعزولة (sandboxed) وإدارتها.

يعرّف `cherino` السمة (trait) [`ContainerOps`] — دورة الحياة الكاملة
للحاوية (الإنشاء، التشغيل، الإيقاف، exec، نسخ الملفات، اللقطات،
الوحدات التخزينية، الصور) — إلى جانب تطبيق مرجعي لـ Docker وطبقة
مشتركة من ملفات الأمان (seccomp وAppArmor وLandlock والتحكم في
الـ egress وقائمة السماح بالسجلّات). ويضيف `cherino-runtime` واجهة
خلفية (backend) أصلية لـ OCI تعمل دون امتيازات الجذر (rootless) مبنية على
[libcontainer](https://github.com/containers/youki).

## بنية الحِزم (crates)

| الحِزمة | الوصف |
|-------|-------------|
| [`cherino-macros`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-macros) | ماكرو الاشتقاق `Getters` المستخدم في أنواع DTO |
| [`cherino`](https://github.com/celestia-island/cherino/tree/master/crates/cherino) | سمة `ContainerOps`، واجهة Docker الخلفية، ملفات الأمان، الأنواع المشتركة |
| [`cherino-runtime`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-runtime) | واجهة OCI الخلفية عبر Youki/libcontainer (خاصة بـ Linux فقط، مع بدائل شكلية للأنظمة الأخرى) |

## الواجهات الخلفية لـ ContainerOps

| الواجهة الخلفية | النوع | المنصة | الآلية | المستوى |
|---------|------|----------|-----------|------|
| **Docker** | API | الجميع | Bollard → Docker Engine HTTP API | أساسي |
| **Youki** | أصلي | Linux | libcontainer → حاويات OCI دون امتيازات الجذر | احتياطي |
| **WSLc** | CLI | Windows | استدعاء صدفي لـ `wslc.exe` / `container.exe` | احتياطي |
| **Apple Container** | CLI | macOS 26+ | أداة `container` السطرية (آلة افتراضية لكل حاوية) | احتياطي |

**Youki من المستوى الاحتياطي**: واجهة libcontainer الخلفية في
`cherino-runtime` تُصان بوصفها بديلاً دون امتيازات الجذر ودون خادم
خلفي (daemon) للمضيفين الذين لا يتوفر لديهم Docker، وليست مشغّل
التنسيق الأساسي. واجهة Docker الخلفية هي الافتراضية وهي المسار
الأكثر اختبارًا في الميدان.

## الميزات (features)

`cherino`:

- `docker` *(افتراضية)* — واجهة `ContainerManager` الخلفية المبنية على Bollard.
- `cli-backend` — محوّلات WSLc / Apple Container السطرية
  (وحدة `cli-backend`). معطّلة افتراضيًا؛ فعّلها صراحةً على مضيفي Windows/macOS.
- `docker-tests` — اختبارات تكامل تتطلب خادم Docker حيًّا
  (معطّلة افتراضيًا؛ تستلزم `docker`).

يبني `cherino-runtime` واجهته الخلفية الخاصة بـ Linux تلقائيًا على
`cfg(target_os = "linux")`؛ أما بقية المنصات فتترجم بديلًا شكليًّا
يعيد أخطاء «متاح فقط على Linux».

## البدء السريع

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
```

```rust
use cherino::{ContainerManager, ops::ContainerOps};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mgr = ContainerManager::new()?; // connects to the local Docker daemon
    let infos = mgr.list().await?;
    for info in infos {
        println!("{}: {:?}", info.name(), info.status());
    }
    Ok(())
}
```

تحميل ملف AppArmor الخاص بـ FUSE الذي تتطلبه الحاويات المتداخلة
(على المضيف، بصلاحيات الجذر، مرة واحدة لكل مضيف):

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

المضيفون الذين ما زالوا يحملون ملف `celestia-plana-fuse` القديم
يُكتشفون ويُقبلون مع تحذير بالإهمال؛ ثبّت الاسم الجديد عندما تستطيع.

## العلامة والتوافقية

استُخرج `cherino` من مساحة عمل `plana`. الأسماء القديمة التالية ما
تزال تُقرأ للتوافقية، مع إصدار `tracing::warn!` لكل منها:

| القديم (plana / entelecheia) | الجديد (cherino) |
|------------------------------|---------------|
| ملف AppArmor `celestia-plana-fuse` | `celestia-cherino-fuse` |
| متغير البيئة `PLANA_APPARMOR_UNCONFINED` | متغير البيئة `CHERINO_APPARMOR_UNCONFINED` |
| متغير البيئة `ENTELECHEIA_RUN_DIR` | متغير البيئة `CHERINO_RUN_DIR` |
| مجلد التشغيل `/tmp/entelecheia/youki` | مجلد التشغيل `/tmp/cherino/youki` |

تحتفظ التجاوزات العامة (`CONTAINER_RUN_DIR` و`CONTAINER_ROOTFS_URL`
و`CONTAINER_NETWORK`) بأسبقيتها على جميع الأسماء ذات العلامة.

## الإفصاح عن التوليد بالذكاء الاصطناعي

الشيفرة في هذا المستودع مولّدة بالذكاء الاصطناعي إلى حد كبير، وهي
مرخّصة بموجب رخصة [SySL-1.0](../../LICENSE). راجع مستودع `sysl`
(<https://github.com/celestia-island/sysl>) للاطلاع على نص الرخصة،
والإفصاح عن النموذج المرفق بملف [LICENSE](../../LICENSE) لهذا
المستودع، والأسئلة الشائعة.

## الرخصة

مرخّص بموجب رخصة [SySL-1.0](../../LICENSE). باستخدامك هذا البرنامج
فأنت تقبل إفصاحه عن التوليد بالذكاء الاصطناعي وشروط الإقرار بالمخاطر.
