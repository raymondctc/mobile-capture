if (
  process.env.MOBILE_CAPTURE_PACKAGE_BUILD !== "1" ||
  !/^[a-f0-9]{64}$/.test(process.env.MOBILE_CAPTURE_PAYLOAD_SHA256 ?? "")
) {
  throw new Error(
    "Complete payload required. Use npm run package:android -- --channel development --cache /absolute/cache --output /absolute/new-output",
  );
}
