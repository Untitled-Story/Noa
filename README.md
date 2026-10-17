# Noa

Noa 是一个用 Rust 编写的 Unity 特效资源编译器，为 Web 运行时 Natsume 提供资源包。

Noa 以 `.prefab` 为入口，自动解析其依赖，将对象层级、动画、粒子系统、材质和纹理转换为统一的中间表示 NOIR，再与资源一起打包为 NOAP (Noa Package)，文件扩展名为 `.noap`。

```text
Unity Prefab → Noa → NOAP (Noa Package) → Natsume
```

自定义 MonoBehaviour 的类型、序列化字段和对象引用也会保留，供宿主通过 Natsume Script Bridge 实现对应行为。Noa 负责资源编译，Natsume 负责运行和渲染。

项目目前处于早期开发阶段，更多 Unity 特性将逐步补充。

## 使用

准备好导出的 Unity YAML 资源及其 `.meta` 文件，指定 Prefab 和输出路径：

```sh
noa compile path/to/effect.prefab --output effect.noap
```

Noa 会自动查找 Prefab 的依赖。如果资源位于其他目录，可以通过 `--search` 添加搜索路径，该参数可以重复使用：

```sh
noa compile path/to/effect.prefab \
  --search path/to/shared-assets \
  --output effect.noap
```

输出的 `.noap` 包含 FlatBuffers 编码的 `.noir` 场景数据、资源清单及所需资产，供 Natsume 加载。

查看资源的依赖和组件信息：

```sh
noa inspect path/to/effect.prefab
```

两个命令均支持 `--json`，以 JSON 格式输出报告。更多参数可以通过 `noa --help` 和对应子命令的 `--help` 查看。

## 从源码编译

需要支持 Rust 2024 edition 的 Rust 工具链。在项目根目录运行：

```sh
cargo build --release --locked
```

编译后的程序位于 `target/release/noa`（Windows 下为 `noa.exe`）。Rust 绑定已包含在仓库中，普通构建无需安装 FlatBuffers 编译器。

使用编译后的程序处理资源：

```sh
./target/release/noa compile \
  path/to/effect.prefab \
  --output out/effect.noap
```

## 许可证

本项目采用 [Apache License 2.0](LICENSE) 许可证。第三方资源保留其原有授权。
