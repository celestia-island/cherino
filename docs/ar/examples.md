# أمثلة

جميع الأمثلة تُترجم مقابل واجهات `cherino` / `cherino-runtime`
البرمجية الحقيقية. أضف الحِزم أولًا:

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
anyhow = "1"
tokio = { version = "1", features = ["full"] }
```

## سرد الحاويات (واجهة Docker الخلفية)

البدء السريع الأدنى: الاتصال بخادم Docker المحلي وسرد الحاويات
العاملة عبر سمة `ContainerOps`.

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

## إنشاء وتشغيل وexec وحذف

دورة الحياة الكاملة عبر `ContainerOps`. يملأ
`ContainerCreateParams::simple` كل حقل بقيمة افتراضية آمنة؛ تجاوز
ما تحتاج إليه فقط.

```rust
use cherino::{ContainerCreateParams, ContainerManager, ops::ContainerOps};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mgr = ContainerManager::new()?;

    let mut params = ContainerCreateParams::simple("cherino-demo", "alpine:latest");
    params.env.insert("GREETING".to_string(), "hello".to_string());

    let info = mgr.create(&params).await?;
    mgr.start(info.id()).await?;

    let out = mgr.exec(info.id(), &["sh", "-c", "echo $GREETING"]).await?;
    println!("exit={:?} stdout={}", out.exit_code, out.stdout.trim());

    mgr.stop(info.id()).await?;
    mgr.remove(info.id(), true).await?;
    Ok(())
}
```

## حاويات OCI دون امتيازات الجذر (واجهة Youki الخلفية)

يطبّق `YoukiManager` من `cherino-runtime` سمة `ContainerOps` نفسها،
دون امتيازات الجذر ودون خادم خلفي، على Linux. تسلسل الاستدعاءات
نفسه يعمل — لا يختلف سوى البناء.

```rust
use std::path::Path;

use cherino::ops::ContainerOps;
use cherino_runtime::YoukiManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mgr = YoukiManager::new(Path::new("/tmp/cherino/youki"))?;
    mgr.initialize().await?;

    let infos = mgr.list().await?;
    println!("{} rootless containers", infos.len());
    Ok(())
}
```

على المنصات غير Linux يترجم `cherino-runtime` بديلًا شكليًّا بالنوع
نفسه وتطبيق السمة نفسه، تعيد استدعاءاته أخطاء «متاح فقط على Linux»،
بحيث تُترجم الشيفرة متعددة المنصات دون تغيير.

## تطبيق ملف أمان

تستخدم أعباء عمل المنصة الملفات الجاهزة من
`cherino::security_profile`. كل `ContainerSecurity` يقابل واحدًا
لواحد حقول `ContainerCreateParams`:

```rust
use cherino::{ContainerCreateParams, ContainerManager, ops::ContainerOps, security_profile};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mgr = ContainerManager::new()?;

    let sec = security_profile::cosmos();
    let mut params = ContainerCreateParams::simple("cosmos-sandbox", "celestia/cosmos:latest");
    params.cap_drop = sec.cap_drop;
    params.cap_add = sec.cap_add;
    params.security_opt = sec.security_opt;
    params.egress_policy = sec.egress_policy;

    let info = mgr.create(&params).await?;
    println!("created hardened container {}", info.id());
    Ok(())
}
```

## سياسة egress مخصصة

ابنِ قائمة سماح للـ egress بأسلوب انسيابي وألحقها بمعاملات الحاوية:

```rust
use cherino::{ContainerCreateParams, EgressPolicy};

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .with_dns_server("192.0.2.53");

let mut params = ContainerCreateParams::simple("net-restricted", "alpine:latest");
params.egress_policy = Some(policy);
```

## ملاحظة عن مدير Snowflake

مدير Snowflake — منسّق العزل الخاص بكل مساحة عمل الذي يشغّل أعباء
عمل الوكلاء — **ليس جزءًا من cherino**: إنه يعيش في الطبقة المستهلِكة
ضمن مساحة عمل entelecheia، بوصفه مستهلكًا لطبقة وقت التشغيل الداخلية
(Cosmos). يختار واجهته الخلفية عبر `ContainerOps`، ويتخلف افتراضيًا
إلى Youki/libcontainer (`COSMOS_CONTAINER_RUNTIME=youki`)، ويضيف
سياسة egress خاصة بكل مساحة عمل فوق
`cherino::security_profile::cosmos()`. النمط الذي يجب تقليده هو ما
سبق: اعتمد على سمة `ContainerOps`، وابنِ أي واجهة خلفية يدعمها
المضيف، وطبّق ملفات الأمان المشتركة.
