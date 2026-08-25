# المقدمة

Cherino هي عدة عمليات الحاويات الخاصة بمنصة celestia: مكتبة Rust
مستقلة توفّر واجهة برمجية موحّدة ومستقلة عن وقت التشغيل لإنشاء
الحاويات المعزولة وإدارتها.

كل عبء عمل معزول في منصة celestia يعمل داخل حاوية مؤقتة معزولة.
والشيفرة المستهلِكة تعتمد دائمًا على سمة `ContainerOps` فقط، ولا تعتمد
أبدًا على وقت تشغيل محدد — ومن ثم تعمل منطق التنسيق نفسه مع Docker،
أو وقت تشغيل OCI دون امتيازات الجذر، أو واجهة خلفية قائمة على سطر
الأوامر.

## ما الذي توفّره

- **سمة `ContainerOps`** — دورة الحياة الكاملة للحاوية: الإنشاء،
  التشغيل، الإيقاف، الحذف، إعادة التشغيل، exec، نسخ الملفات دخولًا
  وخروجًا، لقطات نظام الملفات وفروقاتها، الوحدات التخزينية، والصور.
- **واجهة Docker خلفية مرجعية** — يقود `ContainerManager` واجهة
  Docker Engine HTTP API عبر [Bollard](https://crates.io/crates/bollard)
  (ميزة `docker` الافتراضية).
- **واجهة OCI خلفية دون امتيازات الجذر** — تغلّف حِزمة `cherino-runtime`
  مكتبة [libcontainer](https://github.com/containers/youki) (المكتبة
  التي يقوم عليها وقت تشغيل Youki OCI) كواجهة خلفية دون خادم خلفي
  ودون امتيازات الجذر لمضيفي Linux الذين لا يتوفر لديهم Docker.
- **طبقة مشتركة من ملفات الأمان** — ملفات seccomp، وملف AppArmor
  الخاص بـ FUSE، وقواعد Landlock، وسياسات egress الشبكية، وقائمة
  السماح بالسجلّات، مشتركة بين جميع الواجهات الخلفية بحيث يفرض كل
  وقت تشغيل السياسة نفسها.

## بنية الحِزم

| الحِزمة | الوصف |
|-------|-------------|
| `cherino-macros` | ماكرو الاشتقاق `Getters` المستخدم في أنواع DTO |
| `cherino` | سمة `ContainerOps`، واجهة Docker الخلفية، ملفات الأمان، الأنواع المشتركة |
| `cherino-runtime` | واجهة OCI الخلفية عبر Youki/libcontainer (خاصة بـ Linux فقط، مع بدائل شكلية للأنظمة الأخرى) |

## التثبيت

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
```

أدنى استخدام مقابل خادم Docker المحلي:

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

## ملف AppArmor

الحاويات المتداخلة (الحاويات المنشأة من داخل حاوية) تحتاج إلى تثبيت
ملف AppArmor الخاص بـ FUSE على المضيف، بصلاحيات الجذر، مرة واحدة
لكل مضيف:

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

المضيفون الذين ما زالوا يحملون ملف `celestia-plana-fuse` القديم
يُكتشفون ويُقبلون مع تحذير بالإهمال؛ ثبّت الاسم الجديد عندما تستطيع.
