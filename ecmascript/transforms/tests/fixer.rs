#![feature(box_syntax)]
#![feature(box_patterns)]
#![feature(specialization)]
#![feature(test)]
extern crate sourcemap;
extern crate swc_common;
extern crate swc_ecma_ast;
extern crate swc_ecma_codegen;
extern crate swc_ecma_parser;
extern crate swc_ecma_transforms;
extern crate test;
extern crate testing;
use sourcemap::SourceMapBuilder;
use std::{
    env,
    fs::{read_dir, File},
    io::{self, Read, Write},
    path::Path,
    sync::{Arc, RwLock},
};
use swc_common::{Fold, FoldWith};
use swc_ecma_ast::*;
use swc_ecma_codegen::Emitter;
use swc_ecma_parser::{Parser, Session, SourceFileInput, Syntax};
use swc_ecma_transforms::fixer;
use test::{test_main, Options, ShouldPanic::No, TestDesc, TestDescAndFn, TestFn, TestName};

const IGNORED_PASS_TESTS: &[&str] = &[
    // TODO: uningnore
    "5654d4106d7025c2.js",
    "431ecef8c85d4d24.js",
    // Generated code is better than it from `pass`
    "0da4b57d03d33129.js",
    "aec65a9745669870.js",
    "1c055d256ec34f17.js",
    "d57a361bc638f38c.js",
    "95520bedf0fdd4c9.js",
    "5f1e0eff7ac775ee.js",
    "90ad0135b905a622.js",
    "7da12349ac9f51f2.js",
    "46173461e93df4c2.js",
    "446ffc8afda7e47f.js",
    "3b5d1fb0e093dab8.js",
    "0140c25a4177e5f7.module.js",
    "e877f5e6753dc7e4.js",
    "aac70baa56299267.js",
    // Wrong tests (normalized expected.js is wrong)
    "50c6ab935ccb020a.module.js",
    "9949a2e1a6844836.module.js",
    "1efde9ddd9d6e6ce.module.js",
    // Wrong tests (variable name or value is different)
    "8386fbff927a9e0e.js",
    "0339fa95c78c11bd.js",
    "0426f15dac46e92d.js",
    "0b4d61559ccce0f9.js",
    "0f88c334715d2489.js",
    "1093d98f5fc0758d.js",
    "15d9592709b947a0.js",
    "2179895ec5cc6276.js",
    "247a3a57e8176ebd.js",
    "441a92357939904a.js",
    "47f974d6fc52e3e4.js",
    "4e1a0da46ca45afe.js",
    "5829d742ab805866.js",
    "589dc8ad3b9aa28f.js",
    "598a5cedba92154d.js",
    "72d79750e81ef03d.js",
    "7788d3c1e1247da9.js",
    "7b72d7b43bedc895.js",
    "7dab6e55461806c9.js",
    "82c827ccaecbe22b.js",
    "87a9b0d1d80812cc.js",
    "8c80f7ee04352eba.js",
    "96f5d93be9a54573.js",
    "988e362ed9ddcac5.js",
    "9bcae7c7f00b4e3c.js",
    "a8a03a88237c4e8f.js",
    "ad06370e34811a6a.js",
    "b0fdc038ee292aba.js",
    "b62c6dd890bef675.js",
    "cb211fadccb029c7.js",
    "ce968fcdf3a1987c.js",
    "db3c01738aaf0b92.js",
    "e1387fe892984e2b.js",
    "e71c1d5f0b6b833c.js",
    "e8ea384458526db0.js",
    // We don't implement Annex B fully.
    "1c1e2a43fe5515b6.js",
    "3dabeca76119d501.js",
    "52aeec7b8da212a2.js",
    "59ae0289778b80cd.js",
    "a4d62a651f69d815.js",
    "c06df922631aeabc.js",
];

fn add_test<F: FnOnce() + Send + 'static>(
    tests: &mut Vec<TestDescAndFn>,
    name: String,
    ignore: bool,
    f: F,
) {
    tests.push(TestDescAndFn {
        desc: TestDesc {
            name: TestName::DynTestName(name),
            ignore,
            should_panic: No,
            allow_fail: false,
        },
        testfn: TestFn::DynTestFn(box f),
    });
}

struct MyHandlers;

impl swc_ecma_codegen::Handlers for MyHandlers {}

fn error_tests(tests: &mut Vec<TestDescAndFn>) -> Result<(), io::Error> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("parser")
        .join("tests")
        .join("test262-parser");

    eprintln!("Loading tests from {}", dir.display());

    let normal = dir.join("pass");
    let explicit = dir.join("pass-explicit");

    for entry in read_dir(&explicit).expect("failed to read directory") {
        let entry = entry?;

        let file_name = entry
            .path()
            .strip_prefix(&explicit)
            .expect("failed to strip prefix")
            .to_str()
            .expect("to_str() failed")
            .to_string();

        let input = {
            let mut buf = String::new();
            File::open(entry.path())?.read_to_string(&mut buf)?;
            buf
        };

        let ignore = IGNORED_PASS_TESTS.contains(&&*file_name);

        let module = file_name.contains("module");

        let name = format!("fixer::{}", file_name);

        add_test(tests, name, ignore, {
            let normal = normal.clone();
            move || {
                eprintln!(
                    "\n\n========== Running fixer test {}\nSource:\n{}\n",
                    file_name, input
                );
                let mut wr = Buf(Arc::new(RwLock::new(vec![])));
                let mut wr2 = Buf(Arc::new(RwLock::new(vec![])));

                ::testing::run_test(false, |cm, handler| {
                    let src = cm.load_file(&entry.path()).expect("failed to load file");
                    let expected = cm
                        .load_file(&normal.join(file_name))
                        .expect("failed to load reference file");

                    {
                        let handlers = box MyHandlers;
                        let handlers2 = box MyHandlers;
                        let mut parser: Parser<SourceFileInput> = Parser::new(
                            Session { handler: &handler },
                            Syntax::default(),
                            (&*src).into(),
                            None,
                        );

                        let mut emitter = Emitter {
                            cfg: swc_ecma_codegen::Config { minify: false },
                            cm: cm.clone(),
                            wr: box swc_ecma_codegen::text_writer::JsWriter::new(
                                cm.clone(),
                                "\n",
                                &mut wr,
                                None,
                            ),
                            comments: None,
                            handlers,
                            pos_of_leading_comments: Default::default(),
                        };
                        let mut expected_emitter = Emitter {
                            cfg: swc_ecma_codegen::Config { minify: false },
                            cm: cm.clone(),
                            wr: box swc_ecma_codegen::text_writer::JsWriter::new(
                                cm.clone(),
                                "\n",
                                &mut wr2,
                                None,
                            ),
                            comments: None,
                            handlers: handlers2,
                            pos_of_leading_comments: Default::default(),
                        };

                        // Parse source

                        let mut e_parser: Parser<SourceFileInput> = Parser::new(
                            Session { handler: &handler },
                            Syntax::default(),
                            (&*expected).into(),
                            None,
                        );

                        if module {
                            let module = parser
                                .parse_module()
                                .map(normalize)
                                .map(|p| p.fold_with(&mut fixer()))
                                .map_err(|mut e| {
                                    e.emit();
                                    ()
                                })?;
                            let module2 = e_parser
                                .parse_module()
                                .map(normalize)
                                .map_err(|mut e| {
                                    e.emit();
                                    ()
                                })
                                .expect("failed to parse reference file");
                            if module == module2 {
                                return Ok(());
                            }
                            emitter.emit_module(&module).unwrap();
                            expected_emitter.emit_module(&module2).unwrap();
                        } else {
                            let script = parser
                                .parse_script()
                                .map(normalize)
                                .map(|p| p.fold_with(&mut fixer()))
                                .map_err(|mut e| {
                                    e.emit();
                                    ()
                                })?;
                            let script2 = e_parser
                                .parse_script()
                                .map(normalize)
                                .map(|p| p.fold_with(&mut fixer()))
                                .map_err(|mut e| {
                                    e.emit();
                                    ()
                                })?;

                            if script == script2 {
                                return Ok(());
                            }
                            emitter.emit_script(&script).unwrap();
                            expected_emitter.emit_script(&script2).unwrap();
                        }
                    }

                    let output = String::from_utf8_lossy(&*wr.0.read().unwrap()).to_string();
                    let expected = String::from_utf8_lossy(&*wr2.0.read().unwrap()).to_string();
                    if output == expected {
                        return Ok(());
                    }
                    eprintln!("Wrong output:\n{}\n-----\n{}", output, expected);

                    Err(())
                })
                .expect("failed to run test");
            }
        });
    }

    Ok(())
}

#[test]
fn identity() {
    let args: Vec<_> = env::args().collect();
    let mut tests = Vec::new();
    error_tests(&mut tests).expect("failed to load testss");
    test_main(&args, tests, Options::new());
}

#[derive(Debug, Clone)]
struct Buf(Arc<RwLock<Vec<u8>>>);
impl Write for Buf {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0.write().unwrap().write(data)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.write().unwrap().flush()
    }
}

struct Normalizer;
impl Fold<Stmt> for Normalizer {
    fn fold(&mut self, stmt: Stmt) -> Stmt {
        let stmt = stmt.fold_children(self);

        match stmt {
            Stmt::Expr(box Expr::Paren(ParenExpr { expr, .. })) => Stmt::Expr(expr),
            _ => stmt,
        }
    }
}

impl Fold<PropName> for Normalizer {
    fn fold(&mut self, name: PropName) -> PropName {
        let name = name.fold_children(self);

        match name {
            PropName::Ident(i) => PropName::Str(Str {
                value: i.sym,
                span: i.span,
                has_escape: false,
            }),
            PropName::Num(n) => {
                let s = if n.value.is_infinite() {
                    if n.value.is_sign_positive() {
                        "Infinity".into()
                    } else {
                        "-Infinity".into()
                    }
                } else {
                    format!("{}", n.value)
                };
                PropName::Str(Str {
                    value: s.into(),
                    span: n.span,
                    has_escape: false,
                })
            }
            _ => name,
        }
    }
}

impl Fold<NewExpr> for Normalizer {
    fn fold(&mut self, expr: NewExpr) -> NewExpr {
        let mut expr = expr.fold_children(self);

        expr.args = match expr.args {
            Some(..) => expr.args,
            None => Some(vec![]),
        };

        expr
    }
}

fn normalize<T>(node: T) -> T
where
    T: FoldWith<Normalizer> + FoldWith<::testing::DropSpan>,
{
    node.fold_with(&mut Normalizer)
        .fold_with(&mut ::testing::DropSpan)
}
