# Images in Spark

Spark never generates images on its own initiative. Add images only when the user asked for imagery or the design clearly needs a placeholder they approved.

## Placing an image you already have

- `insert_image` takes a local path (inside a folder the user granted), an http(s) URL, or base64 data, and returns the new layer's id.
- In `write_html`, `<img src="…">` and `background-image: url(…)` accept `data:` URIs, http(s) URLs and `spark-asset:///absolute/path.png` for granted folders.
- Images are stored inside the `.spark` file, so the design keeps working offline.

## Image requests: works with any agent

If you can make images yourself (your own image tool, a script, a model you can call), use an image request:

1. Put a placeholder in the design: `<img src="spark-gen://agent?prompt=Sunlit%20desk%20with%20a%20laptop&aspect_ratio=16:9" style="width: 100%">`.
2. Call `list_image_requests` to see pending requests with their prompts, aspect ratios and reference layers.
3. Make the image, then call `fulfill_image_request` with the `requestId` and one of `path`, `url` or `base64`. The placeholder turns into the image.

## Built-in providers

When the user has enabled a provider in Spark's settings (with their own key), you can request generation directly:

`spark-gen://<provider>/<model>?prompt=…&aspect_ratio=1:1&reference_nodes=12-0,13-0`

- Providers: `openai`, `gemini`, `fal`, `replicate`, `openrouter`, `comfyui`, `automatic1111`.
- The call returns right away. Poll `get_node_info` and read `imageGeneration.status`: `processing`, then `ready` or `error`.
- A single call may request at most the per-call limit set in settings (10 by default), and the monthly budget, if set, applies to the whole batch. Over the limit, the whole call is rejected and nothing is generated.
- Prompts should describe the subject, composition, lighting and style. Reference layers add their text and images as context.
