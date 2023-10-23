
# BeamSEL

A helper application to connect multiple [SecureEpilinkers](https://github.com/medicalinformatics/SecureEpilinker) across restrictive firewalls using [Beam](https://github.com/samply/beam).

## How is SEL expected to talk to BeamSEL

If SEL gets a url like `beam://$BEAMSEL/frankfurt` from MainSEL it is expected to request `http://$BEAMSEL/$REGULAR_PATH_OF_REMOTE_SEL` with a `BEAM-REMOTE` header set to `frankfurt`.
For ABY socket traffic it is expected connect to `$BEAMSEL:$REGULAR_REMOTE_SEL_PORT` there will be a TCP listener ready to forward traffic based on the port.
